use crate::server::Backend;
use log::info;
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Documentation,
    MarkupContent, MarkupKind,
};

static FIELD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\w+)\s*:").unwrap());

pub async fn handle_completion(
    backend: &Backend,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let start = Instant::now();
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let Some(doc) = backend.document_map.get(uri.as_str()) else {
        return Ok(None);
    };
    let Some(line) = doc.lines().nth(position.line as usize) else {
        return Ok(None);
    };

    let curr_char = line
        .chars()
        .nth(position.character.saturating_sub(1) as usize);
    let prev_char = line
        .chars()
        .nth(position.character.saturating_sub(2) as usize);
    if curr_char == Some(' ') && prev_char != Some(':') {
        return Ok(None);
    }

    let Some(captures) = FIELD_RE.captures(line) else {
        return Ok(None);
    };
    let field_name = captures.get(1).map_or("", |m| m.as_str());

    let mut items = Vec::new();

    // User-defined symbols
    for entry in backend.workspace.symbols.iter() {
        let symbol = entry.value();
        let kind = (&symbol.kind).into();

        if kind != CompletionItemKind::FIELD {
            let label = symbol.info.name.clone();
            let sort_text = if field_name.to_lowercase().contains(&label.to_lowercase()) {
                format!("0_{}", label)
            } else {
                format!("1_{}", label)
            };

            items.push(CompletionItem {
                label,
                sort_text: Some(sort_text),
                kind: Some(kind),
                detail: Some(symbol.type_name().to_string()),
                documentation: symbol.info.documentation.as_ref().map(|doc| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: doc.clone(),
                    })
                }),
                ..Default::default()
            });
        }
    }

    // Built-in symbols
    for item in backend.workspace.builtin_symbols.iter() {
        let (name, symbol) = (item.key(), item.value());
        let sort_text = if field_name.to_lowercase().contains(name) {
            format!("0_{}", name)
        } else {
            format!("1_{}", name)
        };
        items.push(CompletionItem {
            label: name.clone(),
            sort_text: Some(sort_text),
            kind: Some(CompletionItemKind::KEYWORD),
            documentation: symbol.info.documentation.as_ref().map(|doc| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: doc.clone(),
                })
            }),
            ..Default::default()
        });
    }

    info!(
        "completion in {}ms: {} L{}C{} -> {} items",
        start.elapsed().as_millis(),
        &uri.path(),
        position.line + 1,
        position.character + 1,
        &items.len()
    );

    Ok(Some(CompletionResponse::Array(items)))
}
