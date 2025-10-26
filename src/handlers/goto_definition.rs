use crate::analysis::WorkspaceSnapshot;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};

pub async fn handle_goto_definition<'a>(
    snapshot: &WorkspaceSnapshot<'a>,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let Some(resolved) = snapshot.resolve_symbol_at(&uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.builtin {
        return Ok(None);
    }

    Ok(Some(GotoDefinitionResponse::Scalar(
        resolved.target.info.location.clone().into(),
    )))
}
