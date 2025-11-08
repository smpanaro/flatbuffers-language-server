use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tower_lsp_server::lsp_types::Diagnostic;

/// A store for diagnostics that tracks their published state.
#[derive(Debug, Clone, Default, PartialEq)]
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
            let has_changed = old_diags.is_none_or(|d| *d != new_diags);
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

    #[must_use]
    pub fn all(&self) -> &HashMap<PathBuf, Vec<Diagnostic>> {
        &self.per_file
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

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp_server::lsp_types::{Position, Range};

    fn make_diagnostic(message: &str) -> Diagnostic {
        Diagnostic::new_simple(
            Range::new(Position::new(0, 0), Position::new(0, 0)),
            message.to_string(),
        )
    }

    #[test]
    fn test_unchanged_diagnostic() {
        let mut store = DiagnosticStore::default();
        let path = PathBuf::from("a.fbs");
        let diag = make_diagnostic("error");
        let mut diagnostics = HashMap::new();
        diagnostics.insert(path.clone(), vec![diag]);

        store.update(diagnostics.clone());
        assert_eq!(store.mark_published().len(), 1);

        store.update(diagnostics);
        assert!(store.mark_published().is_empty());
    }

    #[test]
    fn test_new_file_is_unpublished() {
        let mut store = DiagnosticStore::default();
        let path = PathBuf::from("a.fbs");
        let mut diagnostics = HashMap::new();
        diagnostics.insert(path.clone(), vec![]);

        store.update(diagnostics);
        let unpublished = store.mark_published();
        assert_eq!(unpublished.len(), 1);
        assert!(unpublished.get(&path).unwrap().is_empty());

        assert!(store.mark_published().is_empty());
    }

    #[test]
    fn test_remove_dir() {
        let mut store = DiagnosticStore::default();
        let dir = PathBuf::from("a");
        let path1 = dir.join("b.fbs");
        let path2 = dir.join("c").join("d.fbs");
        let mut diagnostics = HashMap::new();
        diagnostics.insert(path1.clone(), vec![make_diagnostic("error1")]);
        diagnostics.insert(path2.clone(), vec![make_diagnostic("error2")]);

        store.update(diagnostics);
        assert_eq!(store.mark_published().len(), 2);

        store.remove_dir(&dir);
        assert!(store.per_file.is_empty());
        assert!(store.unpublished.is_empty());
    }

    #[test]
    fn test_mark_published() {
        let mut store = DiagnosticStore::default();
        let path1 = PathBuf::from("a.fbs");
        let path2 = PathBuf::from("b.fbs");
        let mut diagnostics = HashMap::new();
        diagnostics.insert(path1.clone(), vec![make_diagnostic("error1")]);
        diagnostics.insert(path2.clone(), vec![make_diagnostic("error2")]);

        store.update(diagnostics);
        assert_eq!(store.mark_published().len(), 2);
        assert!(store.mark_published().is_empty());
    }
}
