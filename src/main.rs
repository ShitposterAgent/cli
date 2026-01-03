use axum::{Json, Router, extract::State, routing::post};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::sync::Arc;
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InjectRequest {
    #[serde(rename = "tabId")]
    tab_id: i32,
    script: String,
}

struct AppState {
    tx: mpsc::UnboundedSender<serde_json::Value>,
}

#[tokio::main]
async fn main() {
    // Redirect logs to stderr so they don't interfere with stdout (Native Messaging)
    // We'll just use epintln! for simplicity.

    let (tx, mut rx) = mpsc::unbounded_channel::<serde_json::Value>();
    let state = Arc::new(AppState { tx });

    // Handle stdout in a dedicated task to ensure protocol compliance
    tokio::spawn(async move {
        let mut stdout = io::stdout();
        while let Some(msg) = rx.recv().await {
            let msg_str = serde_json::to_string(&msg).unwrap();
            let len = msg_str.len() as u32;

            // Writing to stdout must be exactly [4 bytes length][JSON]
            let header = len.to_ne_bytes();

            if let Err(e) = stdout.write_all(&header) {
                eprintln!("Error writing header to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.write_all(msg_str.as_bytes()) {
                eprintln!("Error writing message to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.flush() {
                eprintln!("Error flushing stdout: {}", e);
                break;
            }
        }
    });

    // HTTP Server
    let app = Router::new()
        .route("/inject", post(inject_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    eprintln!("HTTP Server listening on 127.0.0.1:3000");

    // Run the HTTP server in the background
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Main loop: Read from stdin (messages from Chrome Extension)
    let mut stdin = io::stdin();
    loop {
        let mut len_buf = [0u8; 4];
        if let Err(_) = stdin.read_exact(&mut len_buf) {
            eprintln!("Stdn closed or error reading length");
            break;
        }
        let len = u32::from_ne_bytes(len_buf);
        let mut body = vec![0u8; len as usize];
        if let Err(_) = stdin.read_exact(&mut body) {
            eprintln!("Error reading body");
            break;
        }

        let msg: serde_json::Value = match serde_json::from_slice(&body) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error parsing JSON: {}", e);
                continue;
            }
        };

        eprintln!("Received from Chrome: {:?}", msg);
        // Process message from chrome if needed (e.g. session tracking)
    }
}

async fn inject_handler(
    State(state): State<Arc<AppState>>,
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
