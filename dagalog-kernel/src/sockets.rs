use bytes::Bytes;
use dag_rdf::Datastore;
use zeromq::{Socket, SocketRecv, SocketSend};

use crate::cell::{
    CellType, datalog::execute_datalog, detect_cell_type, rml::execute_rml,
    shacl::execute_validate, sparql::execute_sparql, turtle::execute_turtle,
};
use crate::protocol::{Header, JupyterMessage, encode_message, parse_message, reply_header};
use crate::session::KernelSession;

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
pub struct ConnectionFile {
    pub ip: String,
    pub transport: String,
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    pub key: String,
    pub signature_scheme: String,
    pub kernel_name: String,
}

// ── ZMQ frame conversion ─────────────────────────────────────────────────────

fn zmq_to_frames(msg: zeromq::ZmqMessage) -> Vec<Vec<u8>> {
    msg.into_vec().into_iter().map(|b| b.to_vec()).collect()
}

fn frames_to_zmq(frames: Vec<Vec<u8>>) -> Result<zeromq::ZmqMessage, String> {
    let b: Vec<Bytes> = frames.into_iter().map(Bytes::from).collect();
    b.try_into().map_err(|_| "empty frame list".to_string())
}

// ── Identity extraction ───────────────────────────────────────────────────────

/// Split ZMQ frames into (ids_before_delimiter, rest_including_delimiter).
fn split_ids(frames: &[Vec<u8>]) -> (&[Vec<u8>], &[Vec<u8>]) {
    if let Some(pos) = frames.iter().position(|f| f == b"<IDS|MSG>") {
        (&frames[..pos], &frames[pos..])
    } else {
        (&frames[..0], frames)
    }
}

// ── IOPub send helpers ────────────────────────────────────────────────────────

async fn iopub_send(
    iopub: &mut zeromq::PubSocket,
    topic: &[u8],
    msg_type: &str,
    parent: &Header,
    content: serde_json::Value,
    key: &[u8],
) -> Result<(), String> {
    let hdr = reply_header(parent, msg_type);
    let msg = JupyterMessage {
        header: hdr,
        parent_header: serde_json::to_value(parent).unwrap_or_default(),
        metadata: serde_json::json!({}),
        content,
    };
    let ids = vec![topic.to_vec()];
    let frames = encode_message(&msg, key, &ids).map_err(|e| format!("encode {msg_type}: {e}"))?;
    iopub
        .send(frames_to_zmq(frames)?)
        .await
        .map_err(|e| format!("iopub {msg_type}: {e}"))
}

async fn send_status(
    iopub: &mut zeromq::PubSocket,
    state: &str,
    parent: &Header,
    key: &[u8],
) -> Result<(), String> {
    iopub_send(
        iopub,
        b"status",
        "status",
        parent,
        serde_json::json!({ "execution_state": state }),
        key,
    )
    .await
}

async fn send_stream(
    iopub: &mut zeromq::PubSocket,
    text: &str,
    parent: &Header,
    key: &[u8],
) -> Result<(), String> {
    iopub_send(
        iopub,
        b"stream",
        "stream",
        parent,
        serde_json::json!({ "name": "stdout", "text": text }),
        key,
    )
    .await
}

async fn send_execute_result(
    iopub: &mut zeromq::PubSocket,
    data: serde_json::Value,
    exec_count: u64,
    parent: &Header,
    key: &[u8],
) -> Result<(), String> {
    iopub_send(
        iopub,
        b"execute_result",
        "execute_result",
        parent,
        serde_json::json!({
            "execution_count": exec_count,
            "data": data,
            "metadata": {}
        }),
        key,
    )
    .await
}

async fn send_error_output(
    iopub: &mut zeromq::PubSocket,
    err: &str,
    parent: &Header,
    key: &[u8],
) -> Result<(), String> {
    iopub_send(
        iopub,
        b"error",
        "error",
        parent,
        serde_json::json!({
            "ename": "ExecutionError",
            "evalue": err,
            "traceback": [err]
        }),
        key,
    )
    .await
}

// ── Shell send helpers ────────────────────────────────────────────────────────

async fn shell_reply(
    shell: &mut zeromq::RouterSocket,
    ids: &[Vec<u8>],
    msg_type: &str,
    parent: &Header,
    content: serde_json::Value,
    key: &[u8],
) -> Result<(), String> {
    let hdr = reply_header(parent, msg_type);
    let msg = JupyterMessage {
        header: hdr,
        parent_header: serde_json::to_value(parent).unwrap_or_default(),
        metadata: serde_json::json!({}),
        content,
    };
    let frames = encode_message(&msg, key, ids).map_err(|e| format!("encode {msg_type}: {e}"))?;
    shell
        .send(frames_to_zmq(frames)?)
        .await
        .map_err(|e| format!("shell {msg_type}: {e}"))
}

// ── Cell dispatch ─────────────────────────────────────────────────────────────

enum CellOutput {
    /// Rich multi-mime result (SELECT returns HTML + plain).
    Rich(Vec<(String, String)>),
    /// Plain status text (%%turtle, %%load, %%rml, %%reason, %%datalog).
    Stream(String),
}

fn dispatch_cell(cell_type: CellType, ds: &mut Datastore) -> Result<CellOutput, String> {
    match cell_type {
        CellType::Sparql(code) => execute_sparql(ds, &code).map(CellOutput::Rich),
        CellType::Turtle(src) => execute_turtle(ds, &src).map(CellOutput::Stream),
        CellType::Load(path) => {
            let before = ds.named_graphs.quad_count;
            let file = std::fs::File::open(&path)
                .map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
            let reader = std::io::BufReader::new(file);
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            match ext {
                "trig" => {
                    turtle::parse_trig(ds, reader).map_err(|e| format!("TriG parse error: {e}"))?
                }
                "nt" => turtle::parse_ntriples(ds, reader)
                    .map_err(|e| format!("N-Triples parse error: {e}"))?,
                "nq" => turtle::parse_nquads(ds, reader)
                    .map_err(|e| format!("N-Quads parse error: {e}"))?,
                _ => turtle::parse_turtle(ds, reader)
                    .map_err(|e| format!("Turtle parse error: {e}"))?,
            }
            let added = ds.named_graphs.quad_count - before;
            Ok(CellOutput::Stream(format!(
                "Loaded {} triple{}.",
                added,
                if added == 1 { "" } else { "s" }
            )))
        }
        CellType::Rml(path) => execute_rml(ds, &path).map(CellOutput::Stream),
        CellType::Reason => {
            let before = ds.named_graphs.quad_count;
            let ontology_doc = rdf_owl_translator::rdf2owl(ds);
            let rules = owl2rl2datalog::owl2datalog(&mut ds.resources, &ontology_doc.ontology);
            datalog::evaluate_rules(rules, ds);
            let added = ds.named_graphs.quad_count - before;
            Ok(CellOutput::Stream(format!(
                "Reasoning complete. {} triple{} added.",
                added,
                if added == 1 { "" } else { "s" }
            )))
        }
        CellType::Datalog(src) => execute_datalog(ds, &src).map(CellOutput::Stream),
        CellType::Validate(path) => execute_validate(ds, &path).map(CellOutput::Stream),
    }
}

// ── Execute request ───────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn handle_execute(
    shell: &mut zeromq::RouterSocket,
    iopub: &mut zeromq::PubSocket,
    session: &mut KernelSession,
    key: &[u8],
    ids: &[Vec<u8>],
    req_header: &Header,
    code: &str,
    silent: bool,
) -> Result<(), String> {
    session.execution_count += 1;
    let exec_count = session.execution_count;

    let _ = send_status(iopub, "busy", req_header, key).await;

    let cell_type = detect_cell_type(code);
    let result = dispatch_cell(cell_type, &mut session.datastore);
    let ok = result.is_ok();

    match result {
        Ok(output) if !silent => match output {
            CellOutput::Rich(pairs) => {
                let mut data = serde_json::Map::new();
                for (mime, content) in pairs {
                    data.insert(mime, serde_json::Value::String(content));
                }
                let _ = send_execute_result(
                    iopub,
                    serde_json::Value::Object(data),
                    exec_count,
                    req_header,
                    key,
                )
                .await;
            }
            CellOutput::Stream(text) => {
                let _ = send_stream(iopub, &text, req_header, key).await;
            }
        },
        Ok(_) => {}
        Err(ref e) => {
            let _ = send_error_output(iopub, e, req_header, key).await;
        }
    }

    let status = if ok { "ok" } else { "error" };
    shell_reply(
        shell,
        ids,
        "execute_reply",
        req_header,
        serde_json::json!({
            "status": status,
            "execution_count": exec_count,
            "user_expressions": {}
        }),
        key,
    )
    .await?;

    let _ = send_status(iopub, "idle", req_header, key).await;
    Ok(())
}

// ── Shell message dispatch ────────────────────────────────────────────────────

/// Returns `true` if a shutdown was requested.
async fn handle_shell_message(
    shell: &mut zeromq::RouterSocket,
    iopub: &mut zeromq::PubSocket,
    session: &mut KernelSession,
    key: &[u8],
    frames: Vec<Vec<u8>>,
) -> bool {
    let (ids, _) = split_ids(&frames);
    let ids = ids.to_vec();

    let msg = match parse_message(&frames, key) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("shell parse error: {e}");
            return false;
        }
    };

    eprintln!("dagalog-kernel: shell message: {}", msg.header.msg_type);

    // Per Jupyter protocol, every request must be bracketed by
    // status: busy (before) and status: idle (after) on IOPub.
    // The nudge() waits for BOTH a shell reply AND at least one IOPub
    // message, so omitting these means the kernel never appears "ready".
    let _ = send_status(iopub, "busy", &msg.header, key).await;

    match msg.header.msg_type.as_str() {
        "kernel_info_request" => {
            let _ = shell_reply(
                shell,
                &ids,
                "kernel_info_reply",
                &msg.header,
                serde_json::json!({
                    "status": "ok",
                    "protocol_version": "5.3",
                    "implementation": "dagalog",
                    "implementation_version": "0.1.0",
                    "language_info": {
                        "name": "sparql",
                        "version": "1.2",
                        "file_extension": ".rq",
                        "mimetype": "application/sparql-query"
                    },
                    "banner": "Dagalog SPARQL+RDF kernel 0.1.0",
                    "help_links": []
                }),
                key,
            )
            .await;
        }
        "execute_request" => {
            let code = msg.content["code"].as_str().unwrap_or("").to_string();
            let silent = msg.content["silent"].as_bool().unwrap_or(false);
            // handle_execute sends its own busy/idle pair bracketing the actual
            // work; sending an idle here first would let strict clients (e.g.
            // nbclient) think the cell is already done and stop collecting its
            // output before the real busy/idle/output messages arrive.
            let _ =
                handle_execute(shell, iopub, session, key, &ids, &msg.header, &code, silent).await;
            return false;
        }
        "is_complete_request" => {
            let _ = shell_reply(
                shell,
                &ids,
                "is_complete_reply",
                &msg.header,
                serde_json::json!({ "status": "complete" }),
                key,
            )
            .await;
        }
        "complete_request" => {
            let code = msg.content["code"].as_str().unwrap_or("");
            let cursor_pos = msg.content["cursor_pos"].as_u64().unwrap_or(0) as usize;
            let result = crate::completion::complete(code, cursor_pos);
            let _ = shell_reply(
                shell,
                &ids,
                "complete_reply",
                &msg.header,
                serde_json::json!({
                    "status": "ok",
                    "matches": result.matches,
                    "cursor_start": result.cursor_start,
                    "cursor_end": result.cursor_end,
                    "metadata": {}
                }),
                key,
            )
            .await;
        }
        "inspect_request" => {
            let code = msg.content["code"].as_str().unwrap_or("");
            let cursor_pos = msg.content["cursor_pos"].as_u64().unwrap_or(0) as usize;
            let doc = crate::completion::inspect(code, cursor_pos);
            let _ = shell_reply(
                shell,
                &ids,
                "inspect_reply",
                &msg.header,
                serde_json::json!({
                    "status": "ok",
                    "found": doc.is_some(),
                    "data": doc.map(|d| serde_json::json!({ "text/plain": d })).unwrap_or(serde_json::json!({})),
                    "metadata": {}
                }),
                key,
            )
            .await;
        }
        "shutdown_request" => {
            let restart = msg.content["restart"].as_bool().unwrap_or(false);
            let _ = shell_reply(
                shell,
                &ids,
                "shutdown_reply",
                &msg.header,
                serde_json::json!({ "status": "ok", "restart": restart }),
                key,
            )
            .await;
            let _ = send_status(iopub, "idle", &msg.header, key).await;
            return true;
        }
        other => {
            eprintln!("dagalog-kernel: unhandled shell message type: {other}");
        }
    }

    let _ = send_status(iopub, "idle", &msg.header, key).await;
    false
}

/// Returns `true` if a shutdown was requested.
async fn handle_control_message(
    control: &mut zeromq::RouterSocket,
    key: &[u8],
    frames: Vec<Vec<u8>>,
) -> bool {
    let (ids, _) = split_ids(&frames);
    let ids = ids.to_vec();

    let msg = match parse_message(&frames, key) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("control parse error: {e}");
            return false;
        }
    };

    eprintln!("dagalog-kernel: control message: {}", msg.header.msg_type);

    match msg.header.msg_type.as_str() {
        "kernel_info_request" => {
            let _ = shell_reply(
                control,
                &ids,
                "kernel_info_reply",
                &msg.header,
                serde_json::json!({
                    "status": "ok",
                    "protocol_version": "5.3",
                    "implementation": "dagalog",
                    "implementation_version": "0.1.0",
                    "language_info": {
                        "name": "sparql",
                        "version": "1.2",
                        "file_extension": ".rq",
                        "mimetype": "application/sparql-query"
                    },
                    "banner": "Dagalog SPARQL+RDF kernel 0.1.0",
                    "help_links": []
                }),
                key,
            )
            .await;
            false
        }
        "shutdown_request" => {
            let restart = msg.content["restart"].as_bool().unwrap_or(false);
            let _ = shell_reply(
                control,
                &ids,
                "shutdown_reply",
                &msg.header,
                serde_json::json!({ "status": "ok", "restart": restart }),
                key,
            )
            .await;
            true
        }
        other => {
            eprintln!("dagalog-kernel: unhandled control message type: {other}");
            false
        }
    }
}

// ── Main kernel entry point ───────────────────────────────────────────────────

pub async fn run_kernel(connection_file_path: &std::path::Path) -> Result<(), String> {
    let json_str = std::fs::read_to_string(connection_file_path)
        .map_err(|e| format!("cannot read connection file: {e}"))?;
    let conn: ConnectionFile =
        serde_json::from_str(&json_str).map_err(|e| format!("invalid connection file: {e}"))?;

    let key = conn.key.as_bytes().to_vec();
    let addr = |port: u16| format!("{}://{}:{}", conn.transport, conn.ip, port);

    // Bind all five Jupyter sockets.
    let mut shell = zeromq::RouterSocket::new();
    shell
        .bind(&addr(conn.shell_port))
        .await
        .map_err(|e| format!("shell bind: {e}"))?;

    let mut iopub = zeromq::PubSocket::new();
    iopub
        .bind(&addr(conn.iopub_port))
        .await
        .map_err(|e| format!("iopub bind: {e}"))?;

    let mut control = zeromq::RouterSocket::new();
    control
        .bind(&addr(conn.control_port))
        .await
        .map_err(|e| format!("control bind: {e}"))?;

    // Heartbeat: use RouterSocket because RepSocket's strict 2-frame check rejects
    // bare pings sent by some versions of Python's zmq.REQ. A Router echoes the full
    // multipart frame including the routing identity, which REQ strips on receipt.
    let mut hb = zeromq::RouterSocket::new();
    hb.bind(&addr(conn.hb_port))
        .await
        .map_err(|e| format!("heartbeat bind: {e}"))?;

    // Stdin: bind and discard (dagalog never requests input).
    let mut stdin_sock = zeromq::RouterSocket::new();
    stdin_sock
        .bind(&addr(conn.stdin_port))
        .await
        .map_err(|e| format!("stdin bind: {e}"))?;
    tokio::spawn(async move { while stdin_sock.recv().await.is_ok() {} });

    // Heartbeat: echo every ping immediately.
    tokio::spawn(async move {
        eprintln!("dagalog-kernel: heartbeat task started");
        loop {
            match hb.recv().await {
                Ok(msg) => {
                    // Router prepends peer identity; echo the whole thing back so the
                    // Router can route the reply to the right peer.
                    let _ = hb.send(msg).await;
                }
                Err(e) => {
                    eprintln!("dagalog-kernel: heartbeat recv error: {e}");
                    // keep looping — transient errors should not kill the heartbeat
                }
            }
        }
    });

    let mut session = KernelSession::new();
    let empty = crate::protocol::Header {
        msg_id: String::new(),
        session: String::new(),
        username: String::new(),
        date: String::new(),
        msg_type: String::new(),
        version: "5.3".to_string(),
    };

    eprintln!("dagalog-kernel: all sockets bound, entering message loop");

    // Startup status — may be missed by late subscribers but harmless.
    let _ = send_status(&mut iopub, "starting", &empty, &key).await;
    let _ = send_status(&mut iopub, "idle", &empty, &key).await;

    // SIGINT handler: Jupyter sends SIGINT to interrupt a running cell.
    // Without catching it, the OS default kills our process immediately.
    // We consume each SIGINT in the select loop so the kernel stays alive.
    let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|e| format!("SIGINT handler: {e}"))?;

    loop {
        tokio::select! {
            result = shell.recv() => {
                match result {
                    Ok(zmq_msg) => {
                        let frames = zmq_to_frames(zmq_msg);
                        let shutdown = handle_shell_message(
                            &mut shell, &mut iopub, &mut session, &key, frames,
                        ).await;
                        if shutdown { break; }
                    }
                    Err(e) => {
                        eprintln!("dagalog-kernel: shell recv error: {e}");
                        break;
                    }
                }
            }
            result = control.recv() => {
                match result {
                    Ok(zmq_msg) => {
                        let frames = zmq_to_frames(zmq_msg);
                        let shutdown = handle_control_message(&mut control, &key, frames).await;
                        if shutdown { break; }
                    }
                    Err(e) => {
                        eprintln!("dagalog-kernel: control recv error: {e}");
                        break;
                    }
                }
            }
            // Consume SIGINT without exiting. Jupyter uses SIGINT to interrupt
            // long-running cells; the kernel must survive and remain available.
            _ = sigint.recv() => {
                eprintln!("dagalog-kernel: SIGINT received (interrupt)");
            }
        }
    }

    Ok(())
}
