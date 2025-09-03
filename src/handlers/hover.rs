use crate::analysis::resolve_symbol_at;
use crate::server::Backend;
use log::info;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Hover, HoverContents, HoverParams, MarkupContent, MarkupKind};

pub async fn handle_hover(backend: &Backend, params: HoverParams) -> Result<Option<Hover>> {
    let start = Instant::now();
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;

    let res = resolve_symbol_at(&backend.workspace, &uri, pos).map(|resolved| Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: resolved.target.hover_markdown(),
        }),
        range: Some(resolved.range),
    });

    let elapsed = start.elapsed();
    info!(
        "hover in {}ms: {} L{}C{}",
        elapsed.as_millis(),
        &uri.path(),
        pos.line + 1,
        pos.character + 1
    );
    Ok(res)
}
