//! The held-out probe set. See `evals/fixtures/probes.yaml`.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ProbeFile {
    pub probes: Vec<Probe>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Probe {
    pub text: String,
    /// The objection this should match, or `none` for ordinary call speech
    /// that must fire nothing.
    pub expect: String,
}

impl Probe {
    pub fn expected(&self) -> Option<&str> {
        match self.expect.as_str() {
            "none" => None,
            id => Some(id),
        }
    }
}

impl ProbeFile {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let src =
            std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {path}: {e}"))?;
        Ok(serde_yaml::from_str(&src)?)
    }
}
