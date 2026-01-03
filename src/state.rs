use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InjectRequest {
    #[serde(rename = "tabId")]
    pub tab_id: i32,
    pub script: String,
}

pub struct AppState {
    pub tx: mpsc::UnboundedSender<serde_json::Value>,
}

pub type SharedState = Arc<AppState>;
