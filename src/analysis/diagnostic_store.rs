use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tower_lsp_server::lsp_types::Diagnostic;

/// A store for diagnostics that tracks their published state.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticStore {
    per_file: HashMap<PathBuf, Vec<Diagnostic>>,
    /// File paths with diagnostics that have not been published.
    unpublished: HashSet<PathBuf>,
}

impl DiagnosticStore {
    /// Update the store with the latest diagnostics.
    pub fn update(&mut self, diagnostics: HashMap<PathBuf, Vec<Diagnostic>>) {
        for (path, mut new_diags) in diagnostics {
            new_diags.sort_by(|a, b| {
                a.message
                    .cmp(&b.message)
                    .then_with(|| a.range.start.cmp(&b.range.start))
            });

            let old_diags = self.per_file.get(&path);
            let has_changed = old_diags.map_or(true, |d| *d != new_diags);
            if has_changed {
                self.unpublished.insert(path.clone());
            }

            self.per_file.insert(path, new_diags);
        }
    }

    /// Mark all unpublished diagnostics as published and return them.
    /// Caller takes responsibility for publishing them.
    pub fn mark_published(&mut self) -> HashMap<PathBuf, Vec<Diagnostic>> {
        self.unpublished
            .drain()
            .filter_map(|p| self.per_file.get(&p).map(|ds| (p, ds.clone())))
            .collect()
    }

    pub fn remove(&mut self, path: &Path) {
        self.per_file.remove(path);
        self.unpublished.remove(path);
    }

    pub fn remove_dir(&mut self, dir: &Path) {
        let to_remove: Vec<_> = self
            .per_file
            .keys()
            .chain(self.unpublished.iter())
            .filter(|f| f.starts_with(dir))
            .cloned()
            .collect();

        for f in to_remove {
            self.remove(&f);
        }
    }
}
