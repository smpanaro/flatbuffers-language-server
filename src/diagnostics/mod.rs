use std::{collections::HashMap, path::PathBuf};
use tower_lsp_server::lsp_types::Diagnostic;

pub mod codes;
pub mod duplicate_definition;
pub mod expecting_token;
pub mod generic;
pub mod semantic;
pub mod snake_case_warning;
pub mod undefined_type;

pub trait ErrorDiagnosticHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(PathBuf, Diagnostic)>;
}

pub fn generate_diagnostics_from_error_string(
    error_str: &str,
    content: &str,
) -> HashMap<PathBuf, Vec<Diagnostic>> {
    let mut diagnostics_map: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    let handlers: Vec<Box<dyn ErrorDiagnosticHandler>> = vec![
        Box::new(duplicate_definition::DuplicateDefinitionHandler),
        Box::new(expecting_token::ExpectingTokenHandler),
        Box::new(undefined_type::UndefinedTypeHandler),
        Box::new(snake_case_warning::SnakeCaseWarningHandler),
        Box::new(generic::GenericDiagnosticHandler),
    ];

    for line in error_str.lines() {
        for handler in &handlers {
            if let Some((path, diagnostic)) = handler.handle(line, content) {
                diagnostics_map.entry(path).or_default().push(diagnostic);
                break;
            }
        }
    }
    diagnostics_map
}
