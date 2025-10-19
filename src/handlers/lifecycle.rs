use crate::{
    ext::duration::DurationFormat,
    server::Backend,
    utils::paths::{is_flatbuffer_schema, uri_to_path_buf},
};
use log::{debug, info};
use tokio::time::Instant;
use tower_lsp_server::{
    lsp_types::{
        DidChangeTextDocumentParams, DidChangeWorkspaceFoldersParams, DidCloseTextDocumentParams,
        DidOpenTextDocumentParams, DidSaveTextDocumentParams, InitializeParams, Uri,
    },
    UriExt,
};

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    debug!("opened: {}", params.text_document.uri.path());
    // Not sure why, but we occasionally get non .fbs files.
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }
    let Ok(path) = uri_to_path_buf(&params.text_document.uri) else {
        return;
    };

    backend.document_map.insert(
        path.clone(),
        ropey::Rope::from_str(&params.text_document.text),
    );
    backend.parse_many_and_publish(vec![path]).await;
}

pub async fn handle_did_change(backend: &Backend, mut params: DidChangeTextDocumentParams) {
    debug!("changed: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    let Ok(path) = uri_to_path_buf(&params.text_document.uri) else {
        return;
    };

    let content = params.content_changes.remove(0).text;
    backend
        .document_map
        .insert(path.clone(), ropey::Rope::from_str(&content));
    backend.parse_many_and_publish(vec![path]).await;
}

pub async fn handle_did_save(backend: &Backend, params: DidSaveTextDocumentParams) {
    debug!("saved: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }

    let Ok(path) = uri_to_path_buf(&params.text_document.uri) else {
        return;
    };

    if let Some(text) = params.text {
        backend
            .document_map
            .insert(path.clone(), ropey::Rope::from_str(&text));
    }

    let mut files_to_reparse = vec![path.clone()];
    if let Some(includers) = backend.workspace.file_included_by.get(&path) {
        files_to_reparse.extend(includers.value().clone());
    }

    backend.parse_many_and_publish(files_to_reparse).await;
}

pub async fn handle_did_close(_: &Backend, params: DidCloseTextDocumentParams) {
    debug!("closed: {}", params.text_document.uri.path());
    if !is_flatbuffer_schema(&params.text_document.uri) {
        return;
    }
}

pub async fn handle_initialize(backend: &Backend, params: InitializeParams) {
    for folder in params.workspace_folders.as_deref().unwrap_or_default() {
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            backend.workspace_roots.insert(path);
        }
    }

    if let Some(root_uri) = get_root_uri(&params) {
        if let Ok(path) = uri_to_path_buf(&root_uri) {
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

    backend.scan_workspace().await;

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
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            backend.workspace_roots.remove(&path);
            info!("removed root folder: {}", path.to_string_lossy());
        }
        backend.remove_workspace_folder(&folder.uri).await;
    }

    let mut was_folder_added = false;
    for folder in params.event.added {
        if let Ok(path) = uri_to_path_buf(&folder.uri) {
            if backend.workspace_roots.insert(path.clone()) {
                info!("added root folder: {}", path.to_string_lossy());
                was_folder_added = true;
            }
        }
    }

    if was_folder_added {
        backend.scan_workspace().await;
    }
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
