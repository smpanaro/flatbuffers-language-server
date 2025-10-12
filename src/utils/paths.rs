use std::collections::HashSet;
use std::fs;
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

pub fn canonical_file_url(url: &Url) -> Url {
    url.to_file_path()
        .ok()
        .and_then(|p| fs::canonicalize(p).ok())
        .and_then(|p| Url::from_file_path(p).ok())
        .unwrap_or_else(|| url.clone())
}

pub fn file_path_to_canonical_url(path: &String) -> Option<Url> {
    Url::from_file_path(path)
        .ok()
        .map(|u| canonical_file_url(&u))
}
