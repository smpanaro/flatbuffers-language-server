use crate::symbol_table::Symbol;
use dashmap::DashMap;
use std::collections::HashMap;
use tower_lsp::lsp_types::{Location, Url};

#[derive(Debug)]
pub struct Workspace {
    /// A map from a fully qualified symbol name to the Symbol object for user-defined symbols.
    pub symbols: DashMap<String, Symbol>,

    /// A map for built-in scalar types.
    pub builtin_symbols: HashMap<String, Symbol>,

    /// A map from a file's URI to a list of the fully qualified names of symbols
    /// defined within that file. This is crucial for efficiently updating
    /// the `symbols` map when a file changes.
    pub file_definitions: DashMap<Url, Vec<String>>,

    /// A map to store information about `root_type` declarations, keyed by file URI.
    pub root_types: DashMap<Url, RootTypeInfo>,

    /// A map from a file's URI to a list of files it includes.
    pub file_includes: DashMap<Url, Vec<String>>,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            symbols: DashMap::new(),
            builtin_symbols: HashMap::new(),
            file_definitions: DashMap::new(),
            root_types: DashMap::new(),
            file_includes: DashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RootTypeInfo {
    /// The location of the `root_type` keyword and type name.
    pub location: Location,
    /// The name of the type that is declared as the root type (e.g., "MyTable").
    pub type_name: String,
}
