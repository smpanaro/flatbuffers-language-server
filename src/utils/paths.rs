use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::Url;

pub fn is_flatbuffer_schema(uri: &Url) -> bool {
    uri.to_file_path()
        .map_or(false, |p| is_flatbuffer_schema_path(&p))
}

pub fn is_flatbuffer_schema_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map_or(false, |ext| ext.eq_ignore_ascii_case("fbs"))
}

pub fn get_intermediate_paths(starting_path: &Path, roots: &[PathBuf]) -> HashSet<PathBuf> {
    let mut paths = HashSet::new();
    if let Some(mut current_path) = starting_path.parent() {
        loop {
            paths.insert(current_path.to_path_buf());

            if roots.iter().any(|root| current_path == root.as_path()) {
                break;
            }

            if let Some(parent) = current_path.parent() {
                current_path = parent;
            } else {
                break;
            }
        }
    }
    paths
}
