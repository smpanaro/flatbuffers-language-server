use crate::{ext::duration::DurationFormat, server::Backend, utils::paths::is_flatbuffer_schema};
use log::{debug, info};
use tokio::time::Instant;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, Url,
};

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    debug!("opened: {}", params.text_document.uri.path());
    // Not sure why, but we occasionally get non .fbs files.
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    backend
        .parse_and_publish(params.text_document.uri, Some(params.text_document.text))
        .await;
}

pub async fn handle_did_change(backend: &Backend, mut params: DidChangeTextDocumentParams) {
    debug!("changed: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    let content = params.content_changes.remove(0).text;
    backend.document_map.insert(
        params.text_document.uri.to_string(),
        ropey::Rope::from_str(&content),
    );
    backend
        .parse_and_publish(params.text_document.uri, None)
        .await;
}

pub async fn handle_did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    debug!("saved: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    let mut files_to_reparse = vec![params.text_document.uri.clone()];
    if let Some(includers) = backend
        .workspace
        .file_included_by
        .get(&params.text_document.uri)
    {
        files_to_reparse.extend(includers.value().clone());
    }

    for uri in files_to_reparse {
        backend.parse_and_publish(uri, None).await;
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    debug!("closed: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    backend
        .document_map
        .remove(&params.text_document.uri.to_string());

    let includers = backend
        .workspace
        .file_included_by
        .get(&params.text_document.uri)
        .map(|v| v.value().clone())
        .unwrap_or_default();

    // Remove symbols defined in the closed file
    if let Some((_, old_symbol_keys)) = backend
        .workspace
        .file_definitions
        .remove(&params.text_document.uri)
    {
        for key in old_symbol_keys {
            backend.workspace.symbols.remove(&key);
        }
    }

    backend
        .workspace
        .update_includes(&params.text_document.uri, vec![]);

    // Re-parse files that included the closed file
    for uri in includers {
        backend.parse_and_publish(uri.clone(), None).await;
    }
}

pub async fn handle_initialize(backend: &Backend, params: InitializeParams) {
    if let Some(folders) = &params.workspace_folders {
        for folder in folders {
            if let Ok(path) = folder.uri.to_file_path() {
                backend.workspace_roots.insert(path);
            }
        }
    }
    if let Some(root_uri) = get_root_uri(&params) {
        if let Ok(path) = root_uri.to_file_path() {
            backend.workspace_roots.insert(path);
        }
    }
}

pub async fn handle_initialized(backend: &Backend) {
    let start = Instant::now();
    let roots: Vec<_> = backend
        .workspace_roots
        .iter()
        .map(|r| r.key().clone())
        .collect();
    info!("initial workspace roots: {:?}", roots);

    backend.initialize_workspace().await;

    debug!(
        "initialized scan in {}: {} files",
        start.elapsed().log_str(),
        backend.workspace.file_definitions.len()
    );
}

pub async fn handle_did_change_workspace_folders(
    backend: &Backend,
    params: DidChangeWorkspaceFoldersParams,
) {
    for folder in params.event.removed {
        if let Ok(path) = folder.uri.to_file_path() {
            backend.workspace_roots.remove(&path);
            info!("removed root folder: {}", path.to_string_lossy());
        }
    }

    for folder in params.event.added {
        if let Ok(path) = folder.uri.to_file_path() {
            backend.workspace_roots.insert(path.clone());
            info!("added root folder: {}", path.to_string_lossy());
        }
    }

    backend.scan_workspace().await;
}

#[allow(deprecated)]
fn get_root_uri(params: &InitializeParams) -> Option<Url> {
    // root_path is deprecated in favor of root_uri
    params.root_uri.clone().or_else(|| {
        params
            .root_path
            .as_ref()
            .and_then(|p| Url::from_file_path(p).ok())
    })
}
