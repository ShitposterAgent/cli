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
use notify::{Config, RecursiveMode, Watcher};
use state::{AppState, Script, SharedState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use walkdir::WalkDir;

const DEFAULT_PORT: u16 = 58421;

#[derive(Parser)]
#[command(name = "bgm-controller")]
#[command(about = "BGM God-Mode Brain and Script Orchestrator", long_about = None)]
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
    /// Watch a file or directory for script changes and sync in real-time
    Watch {
        path: String,
        #[arg(short, long, default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// List all active tabs
    Tabs,
}

async fn sync_handler(
    State(state): State<SharedState>,
    Json(script): Json<Script>,
) -> Json<serde_json::Value> {
    let mut scripts = state.scripts.lock().unwrap();
    scripts.insert(script.id.clone(), script.clone());

    // Broadcast to extension
    let _ = state.tx.send(serde_json::json!({
        "type": "sync_script",
        "script": script
    }));

    Json(serde_json::json!({ "status": "synced", "id": script.id }))
}

async fn get_tabs_handler(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let tabs = state.tabs.lock().unwrap();
    Json(serde_json::json!({ "tabs": *tabs }))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();
    let state_inner = Arc::clone(&state);

    // Send existing scripts on connect
    let initial_scripts = {
        let scripts = state_inner.scripts.lock().unwrap();
        scripts.values().cloned().collect::<Vec<Script>>()
    };

    for script in initial_scripts {
        let _ = sender
            .send(Message::Text(
                serde_json::to_string(&serde_json::json!({
                    "type": "sync_script",
                    "script": script
                }))
                .unwrap(),
            ))
            .await;
    }

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
                                }
                            }
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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { port } => {
            let addr_str = format!("127.0.0.1:{}", port);
            if std::net::TcpListener::bind(&addr_str).is_err() {
                eprintln!(
                    "\n❌ ERROR: Port {} in use. Brain is already running.\n",
                    port
                );
                std::process::exit(1);
            }

            let (tx, _) = broadcast::channel::<serde_json::Value>(100);
            let state = Arc::new(AppState {
                tx,
                tabs: Mutex::new(Vec::new()),
                scripts: Mutex::new(std::collections::HashMap::new()),
            });

            let app = Router::new()
                .route("/sync", post(sync_handler))
                .route("/tabs", get(get_tabs_handler))
                .route("/ws", get(ws_handler))
                .layer(CorsLayer::permissive())
                .with_state(Arc::clone(&state));

            eprintln!("[BGM BRAIN] Live on http://{}", addr_str);
            axum::serve(tokio::net::TcpListener::bind(&addr_str).await?, app).await?;
        }
        Commands::Watch { path, port } => {
            let path_buf = PathBuf::from(&path);
            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{}/sync", port);

            println!("[WATCHER] Monitoring: {}", path);

            let sync_file = |p: PathBuf, c: &reqwest::Client, u: &String| {
                if p.extension().map_or(false, |ext| ext == "js") {
                    let content = std::fs::read_to_string(&p).unwrap_or_default();
                    let id = p.file_name().unwrap().to_str().unwrap().to_string();
                    let script = Script {
                        id,
                        content,
                        path: p.to_str().unwrap().to_string(),
                    };
                    let client_inner = c.clone();
                    let url_inner = u.clone();
                    tokio::spawn(async move {
                        let _ = client_inner.post(url_inner).json(&script).send().await;
                        println!("[SYNC] Uploaded: {}", script.id);
                    });
                }
            };

            // Initial sync
            for entry in WalkDir::new(&path_buf).into_iter().filter_map(|e| e.ok()) {
                if entry.path().is_file() {
                    sync_file(entry.path().to_path_buf(), &client, &url);
                }
            }

            // Watch for changes
            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
            let mut watcher = notify::RecommendedWatcher::new(
                move |res| {
                    if let Ok(event) = res {
                        let _ = tx.blocking_send(event);
                    }
                },
                Config::default(),
            )?;

            watcher.watch(&path_buf, RecursiveMode::Recursive)?;

            while let Some(event) = rx.recv().await {
                for p in event.paths {
                    if p.is_file() {
                        sync_file(p, &client, &url);
                    }
                }
            }
        }
        Commands::Tabs => {
            let res = reqwest::get(format!("http://127.0.0.1:{}/tabs", DEFAULT_PORT)).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&res.json::<serde_json::Value>().await?)?
            );
        }
    }
    Ok(())
}
