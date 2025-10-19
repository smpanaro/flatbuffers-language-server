use crate::analysis::resolve_symbol_at;
use crate::server::Backend;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};

pub async fn handle_goto_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let Some(resolved) = resolve_symbol_at(&backend.workspace, &uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.builtin {
        return Ok(None);
    }

    Ok(Some(GotoDefinitionResponse::Scalar(
        resolved.target.info.location.into(),
    )))
}
