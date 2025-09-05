use crate::server::Backend;
use log::debug;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
};

pub async fn handle_did_open(backend: &Backend, params: DidOpenTextDocumentParams) {
    debug!("Opened: {}", params.text_document.uri);
    backend
        .parse_and_discover(params.text_document.uri, Some(params.text_document.text))
        .await;
}

pub async fn handle_did_change(backend: &Backend, mut params: DidChangeTextDocumentParams) {
    debug!("Changed: {}", params.text_document.uri);

    let mut files_to_reparse = vec![params.text_document.uri.clone()];
    if let Some(includers) = backend
        .workspace
        .file_included_by
        .get(&params.text_document.uri)
    {
        files_to_reparse.extend(includers.value().clone());
    }

    let content = params.content_changes.remove(0).text;
    backend
        .document_map
        .insert(params.text_document.uri.to_string(), content.clone());

    for uri in files_to_reparse {
        backend.parse_and_discover(uri, None).await;
    }
}

pub async fn handle_did_close(backend: &Backend, params: DidCloseTextDocumentParams) {
    debug!("closed: {}", params.text_document.uri);
    backend
        .document_map
        .remove(&params.text_document.uri.to_string());

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
        .client
        .publish_diagnostics(params.text_document.uri, vec![], None)
        .await;
}
