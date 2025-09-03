use crate::analysis::resolve_symbol_at;
use crate::server::Backend;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};

pub async fn handle_goto_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    let uri = params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    let Some(resolved) = resolve_symbol_at(&backend.workspace, &uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.location.uri.scheme() == "builtin" {
        return Ok(None);
    }

    Ok(Some(GotoDefinitionResponse::Scalar(
        resolved.target.info.location.clone(),
    )))
}
