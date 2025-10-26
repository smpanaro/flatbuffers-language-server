use crate::analysis::WorkspaceSnapshot;
use crate::ext::duration::DurationFormat;
use log::debug;
use std::collections::HashMap;
use std::time::Instant;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    PrepareRenameResponse, ReferenceContext, ReferenceParams, RenameParams,
    TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};

pub async fn prepare_rename<'a>(
    snapshot: &WorkspaceSnapshot<'a>,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    let uri = &params.text_document.uri;
    let position = params.position;

    let Some(resolved) = snapshot.resolve_symbol_at(uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.builtin {
        return Ok(None);
    }

    Ok(Some(PrepareRenameResponse::Range(resolved.range)))
}

pub async fn rename<'a>(
    snapshot: &WorkspaceSnapshot<'a>,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
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

    let Some(references) = super::references::handle_references(snapshot, reference_params).await?
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
