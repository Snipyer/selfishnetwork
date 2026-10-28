use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NicknameStore {
    #[serde(default)]
    nicknames: HashMap<String, String>,
}

fn config_path() -> Option<PathBuf> {
    let mut dir = dirs::config_dir()?;
    dir.push("selfishnet");
    Some(dir.join("nicknames.json"))
}

pub fn load_nicknames() -> HashMap<String, String> {
    let Some(path) = config_path() else {
        return HashMap::new();
    };
    if !path.exists() {
        return HashMap::new();
    }
    let Ok(contents) = fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(store) = serde_json::from_str::<NicknameStore>(&contents) else {
        return HashMap::new();
    };
    store.nicknames
}

pub fn save_nicknames(nicknames: &HashMap<String, String>) -> Result<()> {
    let Some(path) = config_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }
    let store = NicknameStore {
        nicknames: nicknames.clone(),
    };
    let data = serde_json::to_string_pretty(&store)?;
    fs::write(&path, data).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
