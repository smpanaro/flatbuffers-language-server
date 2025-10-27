use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A graph of the include statement relationships between files.
#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    // TODO: These include transitive dependencies. Should they?
    pub includes: HashMap<PathBuf, Vec<PathBuf>>,
    pub included_by: HashMap<PathBuf, Vec<PathBuf>>,
}

impl DependencyGraph {
    pub fn update(&mut self, path: &Path, included_paths: Vec<PathBuf>) {
        if let Some(old_included_files) = self.includes.remove(path) {
            for old_included_path in old_included_files {
                if let Some(included_by) = self.included_by.get_mut(&old_included_path) {
                    included_by.retain(|x| x != path);
                }
            }
        }

        for included_path in &included_paths {
            self.included_by
                .entry(included_path.clone())
                .or_default()
                .push(path.to_path_buf());
        }

        self.includes.insert(path.to_path_buf(), included_paths);
    }

    pub fn remove(&mut self, path: &Path) -> Vec<PathBuf> {
        if let Some(included_files) = self.includes.remove(path) {
            for included_path in included_files {
                if let Some(included_by) = self.included_by.get_mut(&included_path) {
                    included_by.retain(|x| x != path);
                }
            }
        }

        if let Some(included_by_files) = self.included_by.remove(path) {
            return included_by_files;
        }

        return vec![];
    }
}
