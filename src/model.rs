use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WsState {
    Running,
    Exited,
    None,
}

/// One workspace as emitted by `dev list --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub label: String,
    pub repo: String,
    pub workspace: String,
    pub session: String,
    pub state: WsState,
    #[allow(dead_code)]
    pub path: String,
    pub created: i64,
}

pub fn parse_list(stdout: &[u8]) -> Result<Vec<Workspace>, String> {
    let mut list: Vec<Workspace> =
        serde_json::from_slice(stdout).map_err(|e| e.to_string())?;
    list.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(list)
}

/// Compact relative age: "now", "5m", "2h", "3d".
pub fn age_short(created: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(created);
    let secs = (now - created).max(0);
    if secs < 60 {
        "now".to_string()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}
