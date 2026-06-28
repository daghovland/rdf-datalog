//! Security regression tests for the Jupyter wire-protocol implementation.
//!
//! Issue covered:
//! - [#87](https://github.com/daghovland/rdf-datalog/issues/87) Non-constant-time HMAC comparison

use dagalog_kernel::protocol::{ProtocolError, encode_message, parse_message};

// ── #87: HMAC correctness (implementation must use constant-time comparison) ──

/// Every single-byte corruption of the HMAC signature must be rejected.
///
/// The implementation MUST use `subtle::ConstantTimeEq` (not `==`) to prevent
/// a timing oracle on the session key. See [#87](https://github.com/daghovland/rdf-datalog/issues/87).
#[test]
#[ignore] // #87 https://github.com/daghovland/rdf-datalog/issues/87
fn every_corrupted_signature_byte_is_rejected() {
    use dagalog_kernel::protocol::Header;

    let key = b"test-session-key-12345678";
    let msg = dagalog_kernel::protocol::JupyterMessage {
        header: Header {
            msg_id: "abc".into(),
            session: "sess".into(),
            username: "u".into(),
            date: "1970-01-01T00:00:00Z".into(),
            msg_type: "execute_request".into(),
            version: "5.3".into(),
        },
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({"code": "SELECT 1", "silent": false}),
    };

    let ids = vec![b"id0".to_vec()];
    let mut frames = encode_message(&msg, key, &ids).expect("encode must succeed");

    // Locate the hmac frame (right after the "<IDS|MSG>" delimiter)
    let delim_pos = frames.iter().position(|f| f == b"<IDS|MSG>").unwrap();
    let hmac_idx = delim_pos + 1;

    assert_eq!(
        frames[hmac_idx].len(),
        64,
        "HMAC-SHA256 hex must be 64 bytes"
    );

    // Corrupt each byte position in the signature and verify rejection
    for byte_pos in 0..frames[hmac_idx].len() {
        let original = frames[hmac_idx][byte_pos];
        // Flip one bit
        frames[hmac_idx][byte_pos] = original ^ 0x01;

        let result = parse_message(&frames, key);
        assert!(
            matches!(result, Err(ProtocolError::SignatureMismatch)),
            "corrupt byte at position {byte_pos} must cause SignatureMismatch"
        );

        // Restore
        frames[hmac_idx][byte_pos] = original;
    }
}

#[test]
#[ignore] // #87 https://github.com/daghovland/rdf-datalog/issues/87
fn valid_signature_is_accepted() {
    use dagalog_kernel::protocol::Header;

    let key = b"my-test-key";
    let msg = dagalog_kernel::protocol::JupyterMessage {
        header: Header {
            msg_id: "x".into(),
            session: "s".into(),
            username: "u".into(),
            date: "1970-01-01T00:00:00Z".into(),
            msg_type: "kernel_info_request".into(),
            version: "5.3".into(),
        },
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({}),
    };

    let frames = encode_message(&msg, key, &[]).expect("encode must succeed");
    let decoded = parse_message(&frames, key).expect("valid signature must be accepted");
    assert_eq!(decoded.header.msg_id, "x");
}

#[test]
#[ignore] // #87 https://github.com/daghovland/rdf-datalog/issues/87
fn wrong_key_is_rejected() {
    use dagalog_kernel::protocol::Header;

    let correct_key = b"correct-key";
    let wrong_key = b"wrong-key!!";
    let msg = dagalog_kernel::protocol::JupyterMessage {
        header: Header {
            msg_id: "y".into(),
            session: "s".into(),
            username: "u".into(),
            date: "1970-01-01T00:00:00Z".into(),
            msg_type: "kernel_info_request".into(),
            version: "5.3".into(),
        },
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({}),
    };

    let frames = encode_message(&msg, correct_key, &[]).expect("encode must succeed");
    let result = parse_message(&frames, wrong_key);
    assert!(
        matches!(result, Err(ProtocolError::SignatureMismatch)),
        "wrong key must produce SignatureMismatch"
    );
}

#[test]
#[ignore] // #87 https://github.com/daghovland/rdf-datalog/issues/87
fn empty_key_skips_hmac_check_per_jupyter_spec() {
    use dagalog_kernel::protocol::Header;

    // Jupyter spec: when key is empty, skip HMAC verification entirely
    let msg = dagalog_kernel::protocol::JupyterMessage {
        header: Header {
            msg_id: "z".into(),
            session: "s".into(),
            username: "u".into(),
            date: "1970-01-01T00:00:00Z".into(),
            msg_type: "kernel_info_request".into(),
            version: "5.3".into(),
        },
        parent_header: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: serde_json::json!({}),
    };

    let frames = encode_message(&msg, b"some-key", &[]).expect("encode must succeed");
    let decoded = parse_message(&frames, b"").expect("empty key must skip HMAC check");
    assert_eq!(decoded.header.msg_id, "z");
}
