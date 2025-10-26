use crate::{ext::duration::DurationFormat, server::Backend, utils::paths::uri_to_path_buf};
use log::{debug, info};
use tokio::time::Instant;
use tower_lsp_server::{
    lsp_types::{
        Diagnostic, DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams,
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
        InitializeParams, Uri,
    },
    UriExt,
};

pub async fn handle_did_open(
    backend: &Backend,
    params: DidOpenTextDocumentParams,
) -> Vec<(Uri, Vec<Diagnostic>)> {
    if let Some(path) = backend.documents.handle_did_open(params) {
        backend.analysis.parse_many_and_publish(vec![path]).await
    } else {
        vec![]
    }
}

pub async fn handle_did_change(
    backend: &Backend,
    params: DidChangeTextDocumentParams,
) -> Vec<(Uri, Vec<Diagnostic>)> {
    if let Some(path) = backend.documents.handle_did_change(params) {
        backend.analysis.parse_many_and_publish(vec![path]).await
    } else {
        vec![]
    }
}

pub async fn handle_did_save(
    backend: &Backend,
    params: DidSaveTextDocumentParams,
) -> Vec<(Uri, Vec<Diagnostic>)> {
    if let Some((path, _)) = backend.documents.handle_did_save(params) {
        let snapshot = backend.analysis.snapshot().await;
        let mut files_to_reparse = vec![path.clone()];
        if let Some(includers) = snapshot.dependencies.included_by.get(&path) {
            files_to_reparse.extend(includers.clone());
        }
        backend
            .analysis
            .parse_many_and_publish(files_to_reparse)
            .await
    } else {
        vec![]
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    backend.documents.handle_did_close(params);
}

pub async fn handle_initialize(backend: &Backend, params: InitializeParams) {
    for folder in params.workspace_folders.as_deref().unwrap_or_default() {
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            backend.search_paths.workspace_roots.insert(path);
        }
    }

    if let Some(root_uri) = get_root_uri(&params) {
        if let Ok(path) = uri_to_path_buf(&root_uri) {
            backend.search_paths.workspace_roots.insert(path);
        }
    }
}

pub async fn handle_initialized(backend: &Backend) -> Vec<(Uri, Vec<Diagnostic>)> {
    let start = Instant::now();
    let roots: Vec<_> = backend
        .search_paths
        .workspace_roots
        .iter()
        .map(|r| r.key().clone())
        .collect();
    info!("initial workspace roots: {:?}", roots);

    let diagnostics = backend.analysis.scan_workspace().await;

    let snapshot = backend.analysis.snapshot().await;
    debug!(
        "initialized scan in {}: {} files",
        start.elapsed().log_str(),
        snapshot.symbols.per_file.len()
    );
    diagnostics
}

pub async fn handle_did_change_workspace_folders(
    backend: &Backend,
    params: DidChangeWorkspaceFoldersParams,
) -> Vec<(Uri, Vec<Diagnostic>)> {
    let mut diagnostics = Vec::new();
    for folder in params.event.removed {
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            backend.search_paths.workspace_roots.remove(&path);
            info!("removed root folder: {}", path.to_string_lossy());
        }
        diagnostics.append(&mut backend.analysis.remove_workspace_folder(&folder.uri).await);
    }

    let mut was_folder_added = false;
    for folder in params.event.added {
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            if backend.search_paths.workspace_roots.insert(path.clone()) {
                info!("added root folder: {}", path.to_string_lossy());
                was_folder_added = true;
            }
        }
    }

    if was_folder_added {
        diagnostics.append(&mut backend.analysis.scan_workspace().await);
    }
    diagnostics
}

#[allow(deprecated)]
fn get_root_uri(params: &InitializeParams) -> Option<Uri> {
    // root_path is deprecated in favor of root_uri
    params.root_uri.clone().or_else(|| {
        params
            .root_path
            .as_ref()
            .and_then(|p| Uri::from_file_path(p))
    })
}
