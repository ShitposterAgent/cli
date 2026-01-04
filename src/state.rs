use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Script {
    pub id: String,
    pub content: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabInfo {
    pub id: Option<i32>,
    pub url: Option<String>,
    pub title: Option<String>,
}

pub struct AppState {
    pub tx: tokio::sync::broadcast::Sender<serde_json::Value>,
    pub tabs: Mutex<Vec<TabInfo>>,
    pub scripts: Mutex<HashMap<String, Script>>,
}

pub type SharedState = Arc<AppState>;
