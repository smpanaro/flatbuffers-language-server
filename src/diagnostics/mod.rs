use std::{
    borrow::Cow,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
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
    root_path: &Path,
    root_content: &str,
) -> HashMap<PathBuf, Vec<Diagnostic>> {
    let mut diagnostics_map: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    let handlers: Vec<Box<dyn ErrorDiagnosticHandler>> = vec![
        Box::new(duplicate_definition::DuplicateDefinitionHandler),
        Box::new(expecting_token::ExpectingTokenHandler),
        Box::new(undefined_type::UndefinedTypeHandler),
        Box::new(snake_case_warning::SnakeCaseWarningHandler),
        Box::new(generic::GenericDiagnosticHandler),
    ];

    let mut file_cache: HashMap<PathBuf, String> = HashMap::new();

    for line in error_str.lines() {
        let Some(file_path_str) = line.split(':').next() else {
            continue;
        };

        let Ok(canonical_path) = fs::canonicalize(file_path_str) else {
            continue;
        };

        let content = if canonical_path == root_path {
            Cow::Borrowed(root_content)
        } else {
            Cow::Owned(
                file_cache
                    .entry(canonical_path)
                    .or_insert_with(|| fs::read_to_string(file_path_str).unwrap_or_default())
                    .clone(),
            )
        };

        for handler in &handlers {
            if let Some((path, diagnostic)) = handler.handle(line, &content) {
                diagnostics_map.entry(path).or_default().push(diagnostic);
                break;
            }
        }
    }
    diagnostics_map
}
