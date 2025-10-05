use crate::analysis::resolve_symbol_at;
use crate::ext::duration::DurationFormat;
use crate::server::Backend;
use log::debug;
use std::collections::HashMap;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    PrepareRenameResponse, ReferenceContext, ReferenceParams, RenameParams,
    TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};

pub async fn prepare_rename(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let uri = &params.text_document.uri;
    let position = params.position;

    let Some(resolved) = resolve_symbol_at(&backend.workspace, uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.location.uri.scheme() == "builtin" {
        return Ok(None);
    }

    Ok(Some(PrepareRenameResponse::Range(resolved.range)))
}

pub async fn rename(backend: &Backend, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
    let start = Instant::now();
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let reference_params = ReferenceParams {
        text_document_position: params.text_document_position.clone(),
        work_done_progress_params: params.work_done_progress_params,
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let Some(references) = super::references::handle_references(backend, reference_params).await?
    else {
        return Ok(None);
    };

    let new_name = params.new_name;
    let mut changes = HashMap::new();
    for loc in references {
        changes
            .entry(loc.uri)
            .or_insert_with(Vec::new)
            .push(TextEdit::new(loc.range, new_name.clone()));
    }

    let elapsed = start.elapsed();
    debug!(
        "rename in {}: {} L{}C{} -> {} refs",
        elapsed.log_str(),
        &uri.path(),
        position.line + 1,
        position.character + 1,
        changes.len()
    );

    Ok(Some(WorkspaceEdit::new(changes)))
}
