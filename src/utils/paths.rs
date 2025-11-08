use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use tower_lsp_server::lsp_types::Uri;
use tower_lsp_server::UriExt;

pub fn is_flatbuffer_schema(uri: &Uri) -> bool {
    uri.to_file_path()
        .is_some_and(|p| is_flatbuffer_schema_path(&p))
}

#[must_use]
pub fn is_flatbuffer_schema_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("fbs"))
}

pub fn get_intermediate_paths<P, I>(starting_path: &Path, roots: I) -> HashSet<PathBuf>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let root_set: HashSet<PathBuf> = roots
        .into_iter()
        .map(|p| p.as_ref().to_path_buf())
        .collect();

    let mut paths = HashSet::new();
    if let Some(mut current_path) = starting_path.parent() {
        loop {
            paths.insert(current_path.to_path_buf());

            if root_set.contains(current_path) {
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

/// Convert a `lsp_types::Uri` to `PathBuf`.
/// # Errors
///
/// Will return `Err` if `uri` is not a file path or it cannot be canonicalized
/// (e.g. does not exist on disk).
pub fn uri_to_path_buf(uri: &Uri) -> Result<PathBuf, String> {
    uri.to_file_path()
        .ok_or(format!("URL is not a file path: {uri:?}"))
        .and_then(|p| {
            fs::canonicalize(&p)
                .map_err(|err| format!("Failed to canonicalize path '{}': {err}", p.display()))
        })
}

/// Convert a `PathBuf` to `lsp_types::Uri`.
/// # Errors
///
/// Will return `Err` if `path` does not exist.
pub fn path_buf_to_uri(path: &Path) -> Result<Uri, String> {
    Uri::from_file_path(path).ok_or(format!("Failed to convert path to URL: {}", path.display()))
}
