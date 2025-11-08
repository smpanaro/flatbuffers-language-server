use std::{fs, iter::once, path::PathBuf};

use crate::{ext::duration::DurationFormat, server::Backend, utils::paths::uri_to_path_buf};
use log::{debug, info};
use tokio::time::Instant;
use tower_lsp_server::lsp_types::{
    Diagnostic, DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    InitializeParams,
};

pub async fn handle_did_open(
    backend: &Backend,
    params: &DidOpenTextDocumentParams,
) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    if let Some(path) = backend.documents.handle_did_open(params) {
        backend.analyzer.parse(vec![path]).await
    } else {
        vec![]
    }
}

pub async fn handle_did_change(
    backend: &Backend,
    params: DidChangeTextDocumentParams,
) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    if let Some(path) = backend.documents.handle_did_change(params) {
        backend.analyzer.parse(vec![path]).await
    } else {
        vec![]
    }
}

pub async fn handle_did_save(
    backend: &Backend,
    params: DidSaveTextDocumentParams,
) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    if let Some((path, _)) = backend.documents.handle_did_save(params) {
        let mut files_to_reparse = vec![path.clone()];
        {
            let snapshot = backend.analyzer.snapshot().await;
            if let Some(includers) = snapshot.dependencies.included_by.get(&path) {
                files_to_reparse.extend(includers.clone());
            }
        }
        backend.analyzer.parse(files_to_reparse).await
    } else {
        vec![]
    }
}

pub fn handle_did_close(backend: &Backend, params: &DidCloseTextDocumentParams) {
    backend.documents.handle_did_close(params);
}

pub async fn handle_initialize(backend: &Backend, params: InitializeParams) {
    let roots = params
        .workspace_folders
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|f| uri_to_path_buf(&f.uri).ok())
        .chain(once(get_root_path(&params)))
        .flatten()
        .collect::<Vec<_>>();
    // Important: do not trigger a parse until the client is initialized.
    let mut layout = backend.analyzer.layout.write().await;
    layout.add_roots(roots);
}

pub async fn handle_initialized(backend: &Backend) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    let start = Instant::now();

    let files = {
        let mut layout = backend.analyzer.layout.write().await;
        info!("initial workspace roots: {:?}", layout.workspace_roots);

        layout.discover_files()
    };
    let diagnostics = backend.analyzer.parse(files).await;

    let snapshot = backend.analyzer.snapshot().await;
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
) -> Vec<(PathBuf, Vec<Diagnostic>)> {
    let added = params.event.added.into_iter().map(|e| e.uri).collect();
    let removed = params.event.removed.into_iter().map(|e| e.uri).collect();
    backend
        .analyzer
        .handle_workspace_folder_changes(added, removed)
        .await
}

#[allow(deprecated)]
fn get_root_path(params: &InitializeParams) -> Option<PathBuf> {
    // root_path is deprecated in favor of root_uri
    params.root_uri.as_ref().map_or_else(
        || {
            params
                .root_path
                .as_ref()
                .and_then(|p| fs::canonicalize(p).ok())
        },
        |u| uri_to_path_buf(u).ok(),
    )
}
