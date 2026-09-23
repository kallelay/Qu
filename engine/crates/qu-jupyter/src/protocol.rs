//! The Jupyter wire protocol (messaging spec v5.3): message framing, HMAC
//! signing/verification, and header construction. Deliberately independent
//! of any particular ZMQ socket type — it only knows how to turn a
//! `ZmqMessage`'s frames into a [`Message`] and back.
//!
//! Wire layout (frames, in order), fixed by the spec:
//!   [identity frame(s)...] "<IDS|MSG>" hmac_hex header parent_header
//!   metadata content [extra binary buffer frames...]
//!
//! The identity frames only exist on ROUTER sockets (shell/control/stdin) —
//! ZeroMQ prepends the sender's routing id there automatically on `recv`,
//! and a reply must echo it back verbatim so the frontend's DEALER socket
//! gets it. On the PUB socket (iopub) there is no real identity; callers use
//! that same slot for a topic frame instead (see `kernel.rs::send_iopub`).

use bytes::Bytes;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use sha2::Sha256;
use std::collections::VecDeque;
use zeromq::ZmqMessage;

type HmacSha256 = Hmac<Sha256>;

pub const DELIMITER: &[u8] = b"<IDS|MSG>";
pub const PROTOCOL_VERSION: &str = "5.3";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub msg_id: String,
    pub session: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub date: String,
    pub msg_type: String,
    #[serde(default)]
    pub version: String,
}

impl Header {
    pub fn new(session: &str, msg_type: &str) -> Self {
        Header {
            msg_id: uuid::Uuid::new_v4().to_string(),
            session: session.to_string(),
            username: "qu-jupyter".to_string(),
            date: now_iso8601(),
            msg_type: msg_type.to_string(),
            version: PROTOCOL_VERSION.to_string(),
        }
    }
}

fn now_iso8601() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// One parsed (or about-to-be-sent) Jupyter message.
#[derive(Debug)]
pub struct Message {
    /// ROUTER identity frame(s) on a received message; the topic frame to
    /// send with on iopub. Empty for a message this process originates that
    /// isn't a reply (there are none of those here — every outgoing message
    /// is either a reply or an iopub publish).
    pub identities: Vec<Bytes>,
    pub header: Header,
    pub parent_header: Json,
    pub metadata: Json,
    pub content: Json,
    pub buffers: Vec<Bytes>,
}

impl Message {
    /// Build the reply to this message: same routing identities (so a
    /// ROUTER send reaches the same peer), `parent_header` set to this
    /// message's own header (so the frontend can match request/response),
    /// fresh `msg_id`/`date`.
    pub fn reply(&self, session: &str, msg_type: &str, content: Json) -> Message {
        Message {
            identities: self.identities.clone(),
            header: Header::new(session, msg_type),
            parent_header: serde_json::to_value(&self.header).unwrap_or(Json::Null),
            metadata: json!({}),
            content,
            buffers: Vec::new(),
        }
    }

    pub fn parse(raw: ZmqMessage, key: &[u8]) -> Result<Message, String> {
        let frames: Vec<Bytes> = raw.into_vec();
        let delim_pos = frames
            .iter()
            .position(|f| f.as_ref() == DELIMITER)
            .ok_or("no <IDS|MSG> delimiter in message")?;
        if frames.len() < delim_pos + 6 {
            return Err("message truncated before header/parent_header/metadata/content".into());
        }
        let identities = frames[..delim_pos].to_vec();
        let sig_hex =
            std::str::from_utf8(&frames[delim_pos + 1]).map_err(|e| e.to_string())?.to_string();
        let header_b = frames[delim_pos + 2].clone();
        let parent_b = frames[delim_pos + 3].clone();
        let metadata_b = frames[delim_pos + 4].clone();
        let content_b = frames[delim_pos + 5].clone();
        let buffers = frames[delim_pos + 6..].to_vec();

        if !key.is_empty() {
            let expected = sign(key, &header_b, &parent_b, &metadata_b, &content_b);
            if expected != sig_hex {
                return Err("HMAC signature mismatch — wrong key or corrupted message".into());
            }
        }

        let header: Header =
            serde_json::from_slice(&header_b).map_err(|e| format!("bad header: {e}"))?;
        let parent_header: Json = serde_json::from_slice(&parent_b).unwrap_or(Json::Null);
        let metadata: Json = serde_json::from_slice(&metadata_b).unwrap_or(Json::Null);
        let content: Json = serde_json::from_slice(&content_b).unwrap_or(Json::Null);

        Ok(Message { identities, header, parent_header, metadata, content, buffers })
    }

    pub fn into_zmq(self, key: &[u8]) -> ZmqMessage {
        let header_b = serde_json::to_vec(&self.header).unwrap_or_default();
        let parent_b = serde_json::to_vec(&self.parent_header).unwrap_or_default();
        let metadata_b = serde_json::to_vec(&self.metadata).unwrap_or_default();
        let content_b = serde_json::to_vec(&self.content).unwrap_or_default();
        let sig = sign(key, &header_b, &parent_b, &metadata_b, &content_b);

        let mut parts: VecDeque<Bytes> = VecDeque::new();
        for id in self.identities {
            parts.push_back(id);
        }
        parts.push_back(Bytes::from_static(DELIMITER));
        parts.push_back(Bytes::from(sig.into_bytes()));
        parts.push_back(Bytes::from(header_b));
        parts.push_back(Bytes::from(parent_b));
        parts.push_back(Bytes::from(metadata_b));
        parts.push_back(Bytes::from(content_b));
        for b in self.buffers {
            parts.push_back(b);
        }
        // Always non-empty (at minimum: delimiter + sig + 4 json frames),
        // so this can't hit TryFrom's empty-message rejection.
        ZmqMessage::try_from(parts).expect("qu-jupyter: built an empty ZMQ message")
    }
}

fn sign(key: &[u8], header: &[u8], parent: &[u8], metadata: &[u8], content: &[u8]) -> String {
    if key.is_empty() {
        return String::new();
    }
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(header);
    mac.update(parent);
    mac.update(metadata);
    mac.update(content);
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_zmq_framing_with_signature() {
        let key = b"test-key";
        let header = Header::new("sess-1", "execute_request");
        let msg = Message {
            identities: vec![Bytes::from_static(b"peer-id")],
            header,
            parent_header: json!({}),
            metadata: json!({}),
            content: json!({"code": "x = 1", "silent": false}),
            buffers: vec![],
        };
        let zmsg = Message { ..msg }.into_zmq(key);
        let parsed = Message::parse(zmsg, key).expect("valid signed message parses");
        assert_eq!(parsed.header.msg_type, "execute_request");
        assert_eq!(parsed.content["code"], "x = 1");
        assert_eq!(parsed.identities, vec![Bytes::from_static(b"peer-id")]);
    }

    #[test]
    fn rejects_tampered_signature() {
        let key = b"test-key";
        let header = Header::new("sess-1", "execute_request");
        let msg = Message {
            identities: vec![],
            header,
            parent_header: json!({}),
            metadata: json!({}),
            content: json!({"code": "x = 1"}),
            buffers: vec![],
        };
        let zmsg = msg.into_zmq(key);
        // Sign with the right key, then verify with a different one — must fail.
        let err = Message::parse(zmsg, b"wrong-key").unwrap_err();
        assert!(err.contains("signature"), "expected a signature error, got: {err}");
    }

    #[test]
    fn empty_key_skips_signing_both_ways() {
        let header = Header::new("sess-1", "kernel_info_request");
        let msg = Message {
            identities: vec![],
            header,
            parent_header: json!({}),
            metadata: json!({}),
            content: json!({}),
            buffers: vec![],
        };
        let zmsg = msg.into_zmq(b"");
        let parsed = Message::parse(zmsg, b"").expect("unsigned round trip");
        assert_eq!(parsed.header.msg_type, "kernel_info_request");
    }
}
