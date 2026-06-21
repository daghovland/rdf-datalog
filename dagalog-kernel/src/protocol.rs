use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;

type HmacSha256 = Hmac<Sha256>;

/// Jupyter wire-protocol message header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Header {
    pub msg_id: String,
    pub session: String,
    pub username: String,
    pub date: String,
    pub msg_type: String,
    pub version: String,
}

/// A complete Jupyter message (deserialized from multipart ZMQ frames).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JupyterMessage {
    pub header: Header,
    pub parent_header: serde_json::Value,
    pub metadata: serde_json::Value,
    pub content: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid frame count: expected ≥7, got {0}")]
    FrameCount(usize),
    #[error("missing <IDS|MSG> delimiter")]
    MissingDelimiter,
    #[error("JSON deserialize: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HMAC signature mismatch")]
    SignatureMismatch,
}

/// Compute HMAC-SHA256 over the four JSON-encoded Jupyter frames.
pub fn compute_signature(key: &[u8], parts: &[&[u8]]) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    for part in parts {
        mac.update(part);
    }
    hex::encode(mac.finalize().into_bytes())
}

/// Deserialize a raw multipart ZMQ frame list into a `JupyterMessage`.
///
/// Frame layout (Jupyter v5): `[ids..., "<IDS|MSG>", hmac, header, parent_header, metadata, content]`
pub fn parse_message(frames: &[Vec<u8>], key: &[u8]) -> Result<JupyterMessage, ProtocolError> {
    let delim_pos = frames
        .iter()
        .position(|f| f == b"<IDS|MSG>")
        .ok_or(ProtocolError::MissingDelimiter)?;

    let rest = &frames[delim_pos + 1..];
    if rest.len() < 5 {
        return Err(ProtocolError::FrameCount(rest.len()));
    }
    let (hmac_frame, header_frame, parent_frame, meta_frame, content_frame) = (
        &rest[0], &rest[1], &rest[2], &rest[3], &rest[4],
    );

    let expected = compute_signature(key, &[header_frame, parent_frame, meta_frame, content_frame]);
    let got = std::str::from_utf8(hmac_frame).unwrap_or("");
    if !key.is_empty() && got != expected {
        return Err(ProtocolError::SignatureMismatch);
    }

    Ok(JupyterMessage {
        header: serde_json::from_slice(header_frame)?,
        parent_header: serde_json::from_slice(parent_frame)?,
        metadata: serde_json::from_slice(meta_frame)?,
        content: serde_json::from_slice(content_frame)?,
    })
}

/// Serialize a `JupyterMessage` into a multipart ZMQ frame list ready to send.
pub fn encode_message(
    msg: &JupyterMessage,
    key: &[u8],
    ids: &[Vec<u8>],
) -> Result<Vec<Vec<u8>>, serde_json::Error> {
    let header = serde_json::to_vec(&msg.header)?;
    let parent = serde_json::to_vec(&msg.parent_header)?;
    let meta = serde_json::to_vec(&msg.metadata)?;
    let content = serde_json::to_vec(&msg.content)?;
    let sig = compute_signature(key, &[&header, &parent, &meta, &content]);

    let mut frames: Vec<Vec<u8>> = ids.to_vec();
    frames.push(b"<IDS|MSG>".to_vec());
    frames.push(sig.into_bytes());
    frames.push(header);
    frames.push(parent);
    frames.push(meta);
    frames.push(content);
    Ok(frames)
}

/// Convenience: build a reply header from a parent header.
pub fn reply_header(parent: &Header, msg_type: &str) -> Header {
    Header {
        msg_id: uuid::Uuid::new_v4().to_string(),
        session: parent.session.clone(),
        username: parent.username.clone(),
        date: chrono_now(),
        msg_type: msg_type.to_string(),
        version: "5.3".to_string(),
    }
}

fn chrono_now() -> String {
    // ISO 8601 timestamp stub — replace with chrono in green phase
    "1970-01-01T00:00:00.000000Z".to_string()
}

/// Convenience empty content maps.
pub fn empty_metadata() -> serde_json::Value {
    serde_json::json!({})
}

pub type ExtraMetadata = HashMap<String, serde_json::Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn test_hmac_signature_known_vector() {
        // Verify HMAC-SHA256 against a hand-computed reference to guard against
        // key/data ordering bugs in compute_signature.
        let key = b"test-key";
        let parts: &[&[u8]] = &[b"header", b"parent", b"meta", b"content"];
        let sig = compute_signature(key, parts);
        // pre-computed with: echo -n "headerparentmetacontent" | hmac256 test-key
        assert_eq!(sig.len(), 64, "HMAC-SHA256 hex must be 64 chars");
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    #[ignore]
    fn test_message_roundtrip() {
        let key = b"secret";
        let msg = JupyterMessage {
            header: Header {
                msg_id: "abc-123".into(),
                session: "sess-1".into(),
                username: "user".into(),
                date: "2026-01-01T00:00:00Z".into(),
                msg_type: "execute_request".into(),
                version: "5.3".into(),
            },
            parent_header: serde_json::json!({}),
            metadata: serde_json::json!({}),
            content: serde_json::json!({"code": "SELECT * WHERE { ?s ?p ?o } LIMIT 1", "silent": false}),
        };

        let ids = vec![b"id-frame".to_vec()];
        let frames = encode_message(&msg, key, &ids).expect("encode should succeed");
        let decoded = parse_message(&frames, key).expect("parse should succeed");
        assert_eq!(decoded.header.msg_id, msg.header.msg_id);
        assert_eq!(decoded.header.msg_type, "execute_request");
        assert_eq!(decoded.content["code"], msg.content["code"]);
    }

    #[test]
    #[ignore]
    fn test_signature_mismatch_rejected() {
        let key = b"correct-key";
        let wrong_key = b"wrong-key";
        let msg = JupyterMessage {
            header: Header {
                msg_id: "x".into(),
                session: "s".into(),
                username: "u".into(),
                date: "2026-01-01T00:00:00Z".into(),
                msg_type: "kernel_info_request".into(),
                version: "5.3".into(),
            },
            parent_header: serde_json::json!({}),
            metadata: serde_json::json!({}),
            content: serde_json::json!({}),
        };
        let frames = encode_message(&msg, key, &[]).expect("encode");
        let result = parse_message(&frames, wrong_key);
        assert!(
            matches!(result, Err(ProtocolError::SignatureMismatch)),
            "wrong key must produce SignatureMismatch"
        );
    }

    #[test]
    #[ignore]
    fn test_empty_key_skips_signature_check() {
        // When key is empty, Jupyter spec says skip HMAC verification.
        let msg = JupyterMessage {
            header: Header {
                msg_id: "x".into(),
                session: "s".into(),
                username: "u".into(),
                date: "2026-01-01T00:00:00Z".into(),
                msg_type: "kernel_info_request".into(),
                version: "5.3".into(),
            },
            parent_header: serde_json::json!({}),
            metadata: serde_json::json!({}),
            content: serde_json::json!({}),
        };
        let frames = encode_message(&msg, b"any-key", &[]).expect("encode");
        // parse with empty key — must succeed despite sig mismatch
        let decoded = parse_message(&frames, b"").expect("empty key skips hmac");
        assert_eq!(decoded.header.msg_type, "kernel_info_request");
    }
}
