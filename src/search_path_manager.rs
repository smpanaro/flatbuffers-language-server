use dashmap::DashSet;
use std::path::PathBuf;
use tokio::sync::RwLock;

#[derive(Debug)]
pub struct SearchPathManager {
    pub search_paths: RwLock<Vec<PathBuf>>,
    pub workspace_roots: DashSet<PathBuf>,
}

impl SearchPathManager {
    pub fn new() -> Self {
        Self {
            search_paths: RwLock::new(vec![]),
            workspace_roots: DashSet::new(),
        }
    }
}
