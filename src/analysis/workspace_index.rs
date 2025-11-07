use crate::analysis::diagnostic_store::DiagnosticStore;
use crate::analysis::root_type_store::RootTypeStore;
use crate::analysis::symbol_index::SymbolIndex;
use crate::{analysis::dependency_graph::DependencyGraph, parser::ParseResult};
use std::path::{Path, PathBuf};

/// An index of workspace semantic information.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceIndex {
    pub symbols: SymbolIndex,
    pub dependencies: DependencyGraph,
    pub diagnostics: DiagnosticStore,
    pub root_types: RootTypeStore,
}

impl WorkspaceIndex {
    #[must_use] pub fn new() -> Self {
        Self {
            symbols: SymbolIndex::new(),
            dependencies: DependencyGraph::default(),
            diagnostics: DiagnosticStore::default(),
            root_types: RootTypeStore::default(),
        }
    }

    pub fn update(&mut self, path: &Path, result: ParseResult) {
        // If a parse error occurred and there is no symbol table, we don't want to
        // clear the old symbol table as it may be useful to the user while they are
        // editing (e.g. for completions).
        if let Some(st) = result.symbol_table {
            match result.root_type_info {
                Some(rti) => self.root_types.root_types.insert(path.to_path_buf(), rti),
                None => self.root_types.root_types.remove(path),
            };

            self.symbols.update(path, st);
        }

        self.dependencies.update(path, result.includes.clone());

        let mut diagnostics = result.diagnostics;
        // Absence in parse result implies there were no diagnostics for this file.
        diagnostics.entry(path.to_path_buf()).or_default();

        self.diagnostics.update(diagnostics);
    }

    pub fn remove(&mut self, path: &PathBuf) -> Vec<PathBuf> {
        self.symbols.remove(path);
        self.root_types.root_types.remove(path);
        self.diagnostics.remove(path);

        // Return the affected files.
        self.dependencies.remove(path)
    }
}
