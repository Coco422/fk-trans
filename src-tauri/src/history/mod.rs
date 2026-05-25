use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: i64,
    pub original: String,
    pub translated: String,
    pub source_lang: String,
    pub target_lang: String,
    pub provider: String,
}

fn history_path() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fk-trans");
    std::fs::create_dir_all(&dir).ok();
    dir.join("history.json")
}

fn load_history_from_disk() -> Vec<HistoryEntry> {
    let path = history_path();
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_history_to_disk(entries: &[HistoryEntry]) {
    let path = history_path();
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(path, json);
    }
}

pub struct HistoryStore {
    entries: Mutex<Vec<HistoryEntry>>,
    max_entries: usize,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(load_history_from_disk()),
            max_entries: 500,
        }
    }

    pub fn add(&self, entry: HistoryEntry) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(0, entry);
        if entries.len() > self.max_entries {
            entries.truncate(self.max_entries);
        }
        save_history_to_disk(&entries);
    }

    pub fn get_all(&self) -> Vec<HistoryEntry> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn clear(&self) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.clear();
        save_history_to_disk(&entries);
    }
}
