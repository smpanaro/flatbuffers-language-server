use crate::analysis::WorkspaceSnapshot;
use crate::ext::duration::DurationFormat;
use log::debug;
use std::collections::HashMap;
use std::time::Instant;
use tower_lsp_server::lsp_types::{
    PartialResultParams, PrepareRenameResponse, ReferenceContext, ReferenceParams, RenameParams,
    TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};

pub fn prepare_rename(
    snapshot: &WorkspaceSnapshot<'_>,
    params: &TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let uri = &params.text_document.uri;
    let position = params.position;

    let resolved = snapshot.resolve_symbol_at(uri, position)?;

    if resolved.target.info.builtin {
        return None;
    }

    Some(PrepareRenameResponse::Range(resolved.range))
}

pub fn rename(snapshot: &WorkspaceSnapshot<'_>, params: RenameParams) -> Option<WorkspaceEdit> {
    let start = Instant::now();
    let uri = &params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let reference_params = ReferenceParams {
        text_document_position: params.text_document_position.clone(),
        work_done_progress_params: params.work_done_progress_params,
        partial_result_params: PartialResultParams::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let references = super::references::handle_references(snapshot, reference_params)?;

    let new_name = params.new_name;
    #[allow(clippy::mutable_key_type, reason = "external type definition")]
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

    Some(WorkspaceEdit::new(changes))
}
