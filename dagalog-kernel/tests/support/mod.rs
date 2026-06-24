//! Test-only harness for driving the real `dagalog-kernel` binary over the
//! Jupyter wire protocol. See docs/plans/NOTEBOOK_INTEGRATION_TEST_PLAN.md.

use std::collections::HashMap;
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

use bytes::Bytes;
use dagalog_kernel::protocol::{Header, JupyterMessage, encode_message, parse_message};
use zeromq::{DealerSocket, Socket, SocketRecv, SocketSend, SubSocket};

/// Captured result of one `execute_request`.
#[derive(Debug, Default)]
pub struct ExecuteOutcome {
    /// "ok" or "error", from `execute_reply.content.status`.
    pub status: String,
    /// Plain-text stream output (`%%turtle`/`%%load`/`%%rml`/`%%reason`/`%%datalog`).
    pub stream: Option<String>,
    /// Error message, if the cell raised one.
    pub error: Option<String>,
    /// MIME type → content, for SPARQL cells (`execute_result.content.data`).
    pub rich: Option<HashMap<String, String>>,
}

/// A running `dagalog-kernel` process plus connected shell/iopub sockets.
pub struct KernelHarness {
    child: std::process::Child,
    shell: DealerSocket,
    iopub: SubSocket,
    key: Vec<u8>,
    session: String,
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn zmq_to_frames(msg: zeromq::ZmqMessage) -> Vec<Vec<u8>> {
    msg.into_vec().into_iter().map(|b| b.to_vec()).collect()
}

fn frames_to_zmq(frames: Vec<Vec<u8>>) -> zeromq::ZmqMessage {
    let bytes: Vec<Bytes> = frames.into_iter().map(Bytes::from).collect();
    bytes.try_into().expect("non-empty frame list")
}

fn new_header(msg_type: &str, session: &str) -> Header {
    Header {
        msg_id: uuid::Uuid::new_v4().to_string(),
        session: session.to_string(),
        username: "test".to_string(),
        date: "1970-01-01T00:00:00.000000Z".to_string(),
        msg_type: msg_type.to_string(),
        version: "5.3".to_string(),
    }
}

fn request(msg_type: &str, session: &str, content: serde_json::Value) -> JupyterMessage {
    JupyterMessage {
        header: new_header(msg_type, session),
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content,
    }
}

impl KernelHarness {
    /// Spawn the kernel binary with `current_dir` set to `repo_root` (mirroring
    /// `--ServerApp.root_dir=.`), connect shell (Dealer) and iopub (Sub) sockets,
    /// and perform the kernel_info "nudge" handshake.
    pub async fn start(repo_root: &Path) -> Self {
        let shell_port = free_port();
        let iopub_port = free_port();
        let stdin_port = free_port();
        let control_port = free_port();
        let hb_port = free_port();
        let key = uuid::Uuid::new_v4().to_string();

        let connection = serde_json::json!({
            "ip": "127.0.0.1",
            "transport": "tcp",
            "shell_port": shell_port,
            "iopub_port": iopub_port,
            "stdin_port": stdin_port,
            "control_port": control_port,
            "hb_port": hb_port,
            "key": key,
            "signature_scheme": "hmac-sha256",
            "kernel_name": "dagalog",
        });
        let connection_path =
            std::env::temp_dir().join(format!("dagalog-kernel-test-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &connection_path,
            serde_json::to_string(&connection).expect("serialize connection file"),
        )
        .expect("write connection file");

        let child = std::process::Command::new(env!("CARGO_BIN_EXE_dagalog-kernel"))
            .arg("launch")
            .arg("--connection-file")
            .arg(&connection_path)
            .current_dir(repo_root)
            .spawn()
            .expect("spawn dagalog-kernel");

        let mut shell = DealerSocket::new();
        shell
            .connect(&format!("tcp://127.0.0.1:{shell_port}"))
            .await
            .expect("connect shell socket");

        let mut iopub = SubSocket::new();
        iopub
            .connect(&format!("tcp://127.0.0.1:{iopub_port}"))
            .await
            .expect("connect iopub socket");
        iopub.subscribe("").await.expect("subscribe to all topics");

        let mut harness = Self {
            child,
            shell,
            iopub,
            key: key.into_bytes(),
            session: uuid::Uuid::new_v4().to_string(),
        };
        harness.nudge().await;
        harness
    }

    /// Repeatedly send `kernel_info_request` until both a shell reply and at
    /// least one iopub message are observed, working around the ZMQ PUB/SUB
    /// "slow joiner" race.
    async fn nudge(&mut self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut got_shell = false;
        let mut got_iopub = false;

        while !(got_shell && got_iopub) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "kernel did not respond to kernel_info_request nudge within timeout"
            );

            let msg = request("kernel_info_request", &self.session, serde_json::json!({}));
            let frames = encode_message(&msg, &self.key, &[]).expect("encode kernel_info_request");
            let _ = self.shell.send(frames_to_zmq(frames)).await;

            if !got_shell
                && let Ok(Ok(zmq_msg)) =
                    tokio::time::timeout(Duration::from_millis(200), self.shell.recv()).await
                && parse_message(&zmq_to_frames(zmq_msg), &self.key).is_ok()
            {
                got_shell = true;
            }
            if !got_iopub
                && let Ok(Ok(zmq_msg)) =
                    tokio::time::timeout(Duration::from_millis(200), self.iopub.recv()).await
                && parse_message(&zmq_to_frames(zmq_msg), &self.key).is_ok()
            {
                got_iopub = true;
            }
        }
    }

    /// Run one cell's source through `execute_request` and collect its output.
    pub async fn execute(&mut self, code: &str) -> ExecuteOutcome {
        let msg = request(
            "execute_request",
            &self.session,
            serde_json::json!({ "code": code, "silent": false }),
        );
        let req_msg_id = msg.header.msg_id.clone();
        let frames = encode_message(&msg, &self.key, &[]).expect("encode execute_request");
        self.shell
            .send(frames_to_zmq(frames))
            .await
            .expect("send execute_request");

        let mut outcome = ExecuteOutcome::default();
        let mut got_reply = false;
        let mut got_idle = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);

        while !(got_reply && got_idle) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for execute_request {req_msg_id} to complete"
            );

            tokio::select! {
                shell_result = tokio::time::timeout(Duration::from_millis(300), self.shell.recv()) => {
                    if let Ok(Ok(zmq_msg)) = shell_result
                        && let Ok(reply) = parse_message(&zmq_to_frames(zmq_msg), &self.key)
                        && reply.parent_header["msg_id"] == req_msg_id
                        && reply.header.msg_type == "execute_reply"
                    {
                        outcome.status =
                            reply.content["status"].as_str().unwrap_or("").to_string();
                        got_reply = true;
                    }
                }
                iopub_result = tokio::time::timeout(Duration::from_millis(300), self.iopub.recv()) => {
                    if let Ok(Ok(zmq_msg)) = iopub_result
                        && let Ok(io_msg) = parse_message(&zmq_to_frames(zmq_msg), &self.key)
                        && io_msg.parent_header["msg_id"] == req_msg_id
                    {
                        match io_msg.header.msg_type.as_str() {
                            "stream" => {
                                outcome.stream =
                                    io_msg.content["text"].as_str().map(str::to_string);
                            }
                            "execute_result" => {
                                let data = io_msg.content["data"]
                                    .as_object()
                                    .cloned()
                                    .unwrap_or_default();
                                outcome.rich = Some(
                                    data.into_iter()
                                        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
                                        .collect(),
                                );
                            }
                            "error" => {
                                outcome.error =
                                    io_msg.content["evalue"].as_str().map(str::to_string);
                            }
                            "status" if io_msg.content["execution_state"] == "idle" => {
                                got_idle = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        outcome
    }

    /// Send an arbitrary shell message and wait for the matching shell reply
    /// (by `msg_id`), returning its `content`.
    pub async fn request(&mut self, msg_type: &str, content: serde_json::Value) -> serde_json::Value {
        let msg = request(msg_type, &self.session, content);
        let req_msg_id = msg.header.msg_id.clone();
        let frames = encode_message(&msg, &self.key, &[]).expect("encode request");
        self.shell
            .send(frames_to_zmq(frames))
            .await
            .expect("send request");

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                tokio::time::Instant::now() < deadline,
                "timed out waiting for {msg_type} reply"
            );
            if let Ok(Ok(zmq_msg)) =
                tokio::time::timeout(Duration::from_millis(300), self.shell.recv()).await
                && let Ok(reply) = parse_message(&zmq_to_frames(zmq_msg), &self.key)
                && reply.parent_header["msg_id"] == req_msg_id
            {
                return reply.content;
            }
        }
    }

    /// Request a graceful shutdown.
    pub async fn shutdown(&mut self) {
        let msg = request(
            "shutdown_request",
            &self.session,
            serde_json::json!({ "restart": false }),
        );
        let frames = encode_message(&msg, &self.key, &[]).expect("encode shutdown_request");
        let _ = self.shell.send(frames_to_zmq(frames)).await;

        let _ = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(self.child.try_wait(), Ok(Some(_))) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for KernelHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Read `notebooks/dagalog_intro.ipynb` and return the source of every code
/// cell, in document order.
pub fn notebook_code_cells(repo_root: &Path) -> Vec<String> {
    let path = repo_root.join("notebooks/dagalog_intro.ipynb");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let doc: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("invalid notebook JSON: {e}"));
    doc["cells"]
        .as_array()
        .expect("notebook must have a cells array")
        .iter()
        .filter(|cell| cell["cell_type"] == "code")
        .map(|cell| match &cell["source"] {
            serde_json::Value::Array(lines) => lines
                .iter()
                .map(|l| l.as_str().unwrap_or(""))
                .collect::<String>(),
            serde_json::Value::String(s) => s.clone(),
            other => panic!("unexpected cell source shape: {other:?}"),
        })
        .collect()
}
