use crate::analysis::WorkspaceSnapshot;
use tower_lsp_server::lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};

pub fn handle_goto_definition(
    snapshot: &WorkspaceSnapshot<'_>,
    params: GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let resolved = snapshot.resolve_symbol_at(&uri, position)?;

    if resolved.target.info.builtin {
        return None;
    }

    Some(GotoDefinitionResponse::Scalar(
        resolved.target.info.location.clone().into(),
    ))
}
