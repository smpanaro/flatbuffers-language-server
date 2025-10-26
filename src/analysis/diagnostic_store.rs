use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::Diagnostic;

#[derive(Debug, Clone, Default)]
pub struct DiagnosticStore {
    pub published: HashMap<PathBuf, Vec<Diagnostic>>,
}
