mod protocol;
mod state;

use axum::{Json, Router, extract::State, routing::post};
use state::{AppState, InjectRequest, SharedState};
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

async fn inject_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InjectRequest>,
) -> Json<serde_json::Value> {
    eprintln!("Injecting into tab {}: {}", payload.tab_id, payload.script);

    let msg = serde_json::json!({
        "type": "inject",
        "tabId": payload.tab_id,
        "script": payload.script
    });

    let _ = state.tx.send(msg);

    Json(serde_json::json!({ "status": "sent" }))
}

#[tokio::main]
async fn main() {
    let (tx, mut rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let state = Arc::new(AppState { tx });

    // Handle stdout (Native Messaging)
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(msg) = rx.recv().await {
            if let Err(e) = protocol::write_message(&mut stdout, &msg) {
                eprintln!("Error writing message: {}", e);
                break;
            }
        }
    });

    // HTTP Server
    let app = Router::new()
        .route("/inject", post(inject_handler))
        .layer(CorsLayer::permissive())
        .with_state(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    eprintln!("HTTP Server listening on 127.0.0.1:3000");

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Main loop: Read from stdin
    let mut stdin = io::stdin();
    loop {
        match protocol::read_message(&mut stdin) {
            Ok(msg) => {
                eprintln!("Received: {:?}", msg);
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                eprintln!("Extension disconnected");
                break;
            }
            Err(e) => {
                eprintln!("Protocol error: {}", e);
            }
        }
    }
}
