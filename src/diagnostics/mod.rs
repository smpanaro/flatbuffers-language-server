use std::collections::HashMap;
use tower_lsp::lsp_types::{Diagnostic, Url};

use crate::utils::paths::canonical_file_url;

pub mod codes;
pub mod duplicate_definition;
pub mod expecting_token;
pub mod generic;
pub mod semantic;
pub mod snake_case_warning;
pub mod undefined_type;

pub trait ErrorDiagnosticHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(Url, Diagnostic)>;
}

pub fn generate_diagnostics_from_error_string(
    error_str: &str,
    content: &str,
) -> HashMap<Url, Vec<Diagnostic>> {
    let mut diagnostics_map: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    let handlers: Vec<Box<dyn ErrorDiagnosticHandler>> = vec![
        Box::new(duplicate_definition::DuplicateDefinitionHandler),
        Box::new(expecting_token::ExpectingTokenHandler),
        Box::new(undefined_type::UndefinedTypeHandler),
        Box::new(snake_case_warning::SnakeCaseWarningHandler),
        Box::new(generic::GenericDiagnosticHandler),
    ];

    for line in error_str.lines() {
        for handler in &handlers {
            if let Some((file_uri, diagnostic)) = handler.handle(line, content) {
                diagnostics_map
                    .entry(canonical_file_url(&file_uri))
                    .or_default()
                    .push(diagnostic);
                break;
            }
        }
    }
    diagnostics_map
}
