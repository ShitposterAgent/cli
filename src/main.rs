mod state;

use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use futures_util::{sink::SinkExt, stream::StreamExt};
use state::{AppState, InjectRequest, SharedState};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

const DEFAULT_PORT: u16 = 58421;

#[derive(Parser)]
#[command(name = "bgm-controller")]
#[command(about = "BGM God-Mode Brain and CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the persistent Brain server
    Start {
        #[arg(short, long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Navigate to a URL (tabId 0 is default)
    Navigate {
        url: String,
        #[arg(short, long, default_value = "active")]
        tab: String,
    },
    /// List all active tabs
    Tabs,
    /// Get the results history
    Results,
    /// Capture screenshot of a tab
    Capture {
        #[arg(short, long, default_value = "active")]
        tab: String,
    },
    /// Click a selector
    Click {
        selector: String,
        #[arg(short, long, default_value = "active")]
        tab: String,
    },
}

async fn inject_handler(
    State(state): State<SharedState>,
    Json(payload): Json<InjectRequest>,
) -> Json<serde_json::Value> {
    let msg =
        serde_json::json!({ "type": "inject", "tabId": payload.tab_id, "script": payload.script });
    let _ = state.tx.send(msg);
    Json(serde_json::json!({ "status": "sent" }))
}

async fn get_tabs_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let tabs = state.tabs.lock().unwrap();
    Json(serde_json::json!({ "tabs": *tabs }))
}

async fn navigate_handler(
    State(state): State<SharedState>,
    Json(payload): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let msg = serde_json::json!({
        "type": "navigate",
        "tabId": payload.get("tab_id").unwrap_or(&serde_json::json!("active")),
        "url": payload.get("url").unwrap_or(&serde_json::json!("https://google.com"))
    });
    let _ = state.tx.send(msg);
    Json(serde_json::json!({ "status": "navigation_sent" }))
}

async fn results_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let results = state.results.lock().unwrap();
    Json(serde_json::json!({ "results": *results }))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();
    let state_inner = Arc::clone(&state);

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str()) {
                    match msg_type {
                        "tabs" => {
                            if let Some(tabs_val) = msg.get("tabs") {
                                if let Ok(new_tabs) =
                                    serde_json::from_value::<Vec<state::TabInfo>>(tabs_val.clone())
                                {
                                    let mut tabs = state_inner.tabs.lock().unwrap();
                                    *tabs = new_tabs;
                                    eprintln!("[BGM] Updated Tab List");
                                }
                            }
                        }
                        "injection_result" | "html_result" | "capture_result" => {
                            let mut results = state_inner.results.lock().unwrap();
                            results.push(msg.clone());
                            if results.len() > 100 {
                                results.remove(0);
                            }
                            eprintln!("[BGM] Captured result: {}", msg_type);
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
    eprintln!("[BGM] Extension Disconnected");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { port } => {
            let (tx, _) = broadcast::channel::<serde_json::Value>(100);
            let state = Arc::new(AppState {
                tx,
                tabs: Mutex::new(Vec::new()),
                results: Mutex::new(Vec::new()),
            });

            let app = Router::new()
                .route("/inject", post(inject_handler))
                .route("/tabs", get(get_tabs_handler))
                .route("/navigate", post(navigate_handler))
                .route("/results", get(results_handler))
                .route("/ws", get(ws_handler))
                .layer(CorsLayer::permissive())
                .with_state(Arc::clone(&state));

            let addr = format!("127.0.0.1:{}", port);
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            eprintln!("[BGM BRAIN] Operating on http://{}", addr);
            axum::serve(listener, app).await?;
        }
        _ => {
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}", DEFAULT_PORT);
            match cli.command {
                Commands::Navigate { url: nav_url, tab } => {
                    let res = client
                        .post(format!("{}/navigate", url))
                        .json(&serde_json::json!({ "url": nav_url, "tab_id": tab }))
                        .send()
                        .await?;
                    println!("{:?}", res.json::<serde_json::Value>().await?);
                }
                Commands::Tabs => {
                    let res = client.get(format!("{}/tabs", url)).send().await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res.json::<serde_json::Value>().await?)?
                    );
                }
                Commands::Results => {
                    let res = client.get(format!("{}/results", url)).send().await?;
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&res.json::<serde_json::Value>().await?)?
                    );
                }
                _ => println!("Command not yet implemented in CLI mode"),
            }
        }
    }
    Ok(())
}
