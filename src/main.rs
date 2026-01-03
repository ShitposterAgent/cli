mod protocol;
mod state;

use axum::{
    Json, Router,
    extract::State,
    routing::{get, post},
};
use state::{AppState, InjectRequest, Rule, SharedState};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

async fn inject_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InjectRequest>,
) -> Json<serde_json::Value> {
    let msg = serde_json::json!({
        "type": "inject",
        "tabId": payload.tab_id,
        "script": payload.script
    });

    let _ = state.tx.send(msg);
    Json(serde_json::json!({ "status": "sent", "target": payload.tab_id }))
}

async fn get_tabs_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let tabs = state.tabs.lock().unwrap();
    Json(serde_json::json!({ "tabs": *tabs }))
}

async fn set_rules_handler(
    State(state): State<SharedState>,
    Json(rules): Json<Vec<Rule>>,
) -> Json<serde_json::Value> {
    let msg = serde_json::json!({
        "type": "set_rules",
        "rules": rules
    });
    let _ = state.tx.send(msg);
    Json(serde_json::json!({ "status": "rules_broadcasted", "count": rules.len() }))
}

#[tokio::main]
async fn main() {
    let (tx, mut rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let state = Arc::new(AppState {
        tx,
        tabs: Mutex::new(Vec::new()),
    });

    // Handle stdout (Native Messaging Out)
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(msg) = rx.recv().await {
            if let Err(e) = protocol::write_message(&mut stdout, &msg) {
                eprintln!("[NATIVE] Error writing: {}", e);
                break;
            }
        }
    });

    // HTTP Server
    let app = Router::new()
        .route("/inject", post(inject_handler))
        .route("/tabs", get(get_tabs_handler))
        .route("/rules", post(set_rules_handler))
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    eprintln!("[SERVER] Listening on 127.0.0.1:3000");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Main loop: Read from stdin (Native Messaging In)
    let mut stdin = io::stdin();
    loop {
        match protocol::read_message(&mut stdin) {
            Ok(msg) => {
                if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str()) {
                    match msg_type {
                        "tabs" => {
                            if let Some(tabs_val) = msg.get("tabs") {
                                if let Ok(new_tabs) =
                                    serde_json::from_value::<Vec<state::TabInfo>>(tabs_val.clone())
                                {
                                    let mut tabs = state.tabs.lock().unwrap();
                                    *tabs = new_tabs;
                                    eprintln!("[BGM] Updated Tab List ({} tabs)", tabs.len());
                                }
                            }
                        }
                        "injection_result" => {
                            eprintln!("[BGM] Injection result: {}", msg);
                        }
                        _ => eprintln!("[BGM] Received unknown message type: {}", msg_type),
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                eprintln!("[NATIVE] Extension disconnected");
                break;
            }
            Err(e) => {
                eprintln!("[NATIVE] Protocol error: {}", e);
            }
        }
    }
}
