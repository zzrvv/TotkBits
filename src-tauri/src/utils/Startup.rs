use crate::{utils::update_json, TotkConfig::TotkConfig};
use serde_json::json;
use std::{env, io};

#[tauri::command]
pub fn get_startup_data(
    state: tauri::State<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    Ok((*state.inner()).clone())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StartupData {
    pub argv1: String,
    pub argv: Vec<String>,
    pub config: TotkConfig,
}

impl StartupData {
    pub fn new() -> io::Result<Self> {
        let argv: Vec<String> = env::args().skip(1).collect();
        let argv1 = argv.first().cloned().unwrap_or_default();
        let config = TotkConfig::safe_new(true)?;
        Ok(Self {
            argv1,
            argv,
            config,
        })
    }

    pub fn to_json(&self) -> io::Result<serde_json::Value> {
        Ok(update_json(
            json!({"argv1": self.argv1, "argv": self.argv}),
            self.config.to_react_json()?,
        ))
    }
}
