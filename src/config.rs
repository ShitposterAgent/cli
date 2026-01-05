use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub agents_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 58421,
            agents_dir: "./agents".to_string(),
        }
    }
}

pub fn get_config_dir() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME env var not set");
    let mut path = PathBuf::from(home);
    path.push(".shitposteragent");
    if !path.exists() {
        fs::create_dir_all(&path).expect("Failed to create config directory");
    }
    path
}

pub fn load_config() -> Config {
    let dir = get_config_dir();
    let path = dir.join("config.toml");

    if path.exists() {
        let content = fs::read_to_string(&path).expect("Failed to read config");
        toml::from_str(&content).unwrap_or_else(|e| {
            eprintln!("[CONFIG] Error parsing config.toml: {}. Using defaults.", e);
            Config::default()
        })
    } else {
        let config = Config::default();
        let content = toml::to_string_pretty(&config).expect("Failed to serialize config");
        fs::write(path, content).expect("Failed to write default config");
        config
    }
}
