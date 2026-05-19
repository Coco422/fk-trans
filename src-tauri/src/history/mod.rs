use serde::{Deserialize, Serialize};
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

pub struct HistoryStore {
    entries: Mutex<Vec<HistoryEntry>>,
    max_entries: usize,
}

impl HistoryStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            max_entries: 500,
        }
    }

    pub fn add(&self, entry: HistoryEntry) {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(0, entry);
        if entries.len() > self.max_entries {
            entries.truncate(self.max_entries);
        }
    }

    pub fn get_all(&self) -> Vec<HistoryEntry> {
        self.entries.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}
