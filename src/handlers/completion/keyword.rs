use crate::analysis::WorkspaceSnapshot;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Documentation, MarkupContent,
    MarkupKind,
};

pub fn handle_keyword_completion(
    snapshot: &WorkspaceSnapshot,
    line: &str,
) -> Option<CompletionResponse> {
    let partial_keyword = line.trim();
    let items: Vec<CompletionItem> = snapshot
        .symbols
        .keywords
        .iter()
        .filter(|item| item.0.starts_with(partial_keyword))
        .map(|(name, item)| CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::KEYWORD),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: item.clone(),
            })),
            ..Default::default()
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}
