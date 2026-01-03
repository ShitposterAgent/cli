use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InjectRequest {
    #[serde(rename = "tabId")]
    pub tab_id: serde_json::Value, // Can be i32 or String "all"
    pub script: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabInfo {
    pub id: Option<i32>,
    pub url: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Rule {
    pub id: String,
    pub pattern: String,
    pub script: String,
    pub enabled: bool,
}

pub struct AppState {
    pub tx: mpsc::UnboundedSender<serde_json::Value>,
    pub tabs: Mutex<Vec<TabInfo>>,
    pub results: Mutex<Vec<serde_json::Value>>,
}

pub type SharedState = Arc<AppState>;
