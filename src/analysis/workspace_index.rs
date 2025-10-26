use crate::analysis::dependency_graph::DependencyGraph;
use crate::analysis::diagnostic_store::DiagnosticStore;
use crate::analysis::root_type_store::RootTypeStore;
use crate::analysis::symbol_index::SymbolIndex;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    pub symbols: SymbolIndex,
    pub dependencies: DependencyGraph,
    pub diagnostics: DiagnosticStore,
    pub root_types: RootTypeStore,
}

impl WorkspaceIndex {
    pub fn remove_file(&mut self, path: &PathBuf) -> Vec<PathBuf> {
        if let Some(old_symbol_keys) = self.symbols.per_file.remove(path) {
            for key in old_symbol_keys {
                self.symbols.global.remove(&key);
            }
        }

        if let Some(included_files) = self.dependencies.includes.remove(path) {
            for included_path in included_files {
                if let Some(included_by) = self.dependencies.included_by.get_mut(&included_path) {
                    included_by.retain(|x| x != path);
                }
            }
        }

        self.root_types.root_types.remove(path);
        self.diagnostics.published.remove(path);

        if let Some(included_by_files) = self.dependencies.included_by.remove(path) {
            return included_by_files;
        }

        vec![]
    }

    pub fn expand_to_known_files(&self, path: &PathBuf) -> Vec<PathBuf> {
        let has_ext = path.extension().is_some();
        if has_ext {
            return vec![path.clone()];
        }

        self.symbols
            .per_file
            .keys()
            .filter(|fp| fp.starts_with(path))
            .cloned()
            .collect()
    }

    pub fn new() -> Self {
        Self {
            symbols: SymbolIndex::new(),
            dependencies: DependencyGraph::default(),
            diagnostics: DiagnosticStore::default(),
            root_types: RootTypeStore::default(),
        }
    }
}
