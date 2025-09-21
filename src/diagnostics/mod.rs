use tower_lsp::lsp_types::{Diagnostic, Url};

pub mod duplicate_definition;
pub mod generic;
pub mod undefined_type;

pub trait DiagnosticHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(Url, Diagnostic)>;
}
