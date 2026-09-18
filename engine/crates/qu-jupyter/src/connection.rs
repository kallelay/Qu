//! Parsing for Jupyter's "connection file" — the JSON blob a frontend writes
//! to a temp path and passes to the kernel as `-f <path>` (see `main.rs`).
//! Field names/casing here are fixed by the Jupyter wire-protocol spec, not
//! a choice made here.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionInfo {
    pub shell_port: u16,
    pub iopub_port: u16,
    pub stdin_port: u16,
    pub control_port: u16,
    pub hb_port: u16,
    pub ip: String,
    #[serde(default)]
    pub key: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    // Read from the connection file for completeness (it's part of the
    // wire format) but not branched on: HMAC-SHA256 is the only scheme any
    // real Jupyter frontend has ever written here.
    #[serde(default)]
    #[allow(dead_code)]
    pub signature_scheme: String,
}

fn default_transport() -> String {
    "tcp".to_string()
}

impl ConnectionInfo {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("reading connection file {}: {e}", path.display()))?;
        serde_json::from_str(&text)
            .map_err(|e| format!("parsing connection file {}: {e}", path.display()))
    }

    pub fn endpoint(&self, port: u16) -> String {
        format!("{}://{}:{}", self.transport, self.ip, port)
    }
}
