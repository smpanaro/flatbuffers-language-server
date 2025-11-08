use crate::symbol_table::SymbolKind;
use crate::utils::as_pos_idx;
use crate::{analysis::WorkspaceSnapshot, handlers::completion::util::generate_include_text_edit};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionResponse,
    CompletionTextEdit, Documentation, MarkupContent, MarkupKind, Position, Range, TextEdit,
};

static ROOT_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*root_type\s+([\w\.]*)").unwrap());

pub fn handle_root_type_completion(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    line: &str,
    position: Position,
) -> Option<CompletionResponse> {
    let (range, partial_text) = get_root_type_completion_context(line, position)?;

    let mut items = Vec::new();
    for entry in &snapshot.symbols.global {
        let symbol = entry.1;
        if let SymbolKind::Table(_) = &symbol.kind {
            let symbol = entry.1;
            let base_name = &symbol.info.name;
            let qualified_name = symbol.info.qualified_name();

            let base_match = base_name.starts_with(&partial_text);
            let qualified_match = qualified_name.starts_with(&partial_text);

            if base_match || qualified_match {
                // TODO: Handle name collisions here as well.
                let use_qualified = partial_text.contains('.');
                let new_text = if use_qualified {
                    qualified_name.clone()
                } else {
                    base_name.clone()
                };

                let (additional_text_edits, preview_text) =
                    generate_include_text_edit(snapshot, path, symbol);

                items.push(CompletionItem {
                    label: base_name.clone(),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit { range, new_text })),
                    additional_text_edits,
                    kind: Some(CompletionItemKind::CLASS),
                    detail: Some(symbol.type_name().to_string()),
                    label_details: Some(CompletionItemLabelDetails {
                        detail: None,
                        description: preview_text.or(symbol.info.namespace_str()), // for fully qualified name or file path.
                    }),
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
    }

    for ns in snapshot.symbols.namespaces() {
        if !ns.starts_with(&partial_text) {
            continue;
        }

        items.push(CompletionItem {
            label: ns.clone(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: ns.clone(),
            })),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some("namespace".to_string()),
            ..Default::default()
        });
    }

    Some(CompletionResponse::Array(items))
}

fn get_root_type_completion_context(line: &str, position: Position) -> Option<(Range, String)> {
    let line_upto_cursor = &line[..position.character as usize];
    ROOT_TYPE_RE
        .captures(line_upto_cursor)
        .and_then(|captures| {
            captures.get(1).map(|partial_match| {
                let start_char =
                    as_pos_idx(line_upto_cursor[..partial_match.start()].chars().count());
                let range = Range {
                    start: Position {
                        line: position.line,
                        character: start_char,
                    },
                    end: position,
                };
                let partial_text = partial_match.as_str().to_string();
                (range, partial_text)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_root_type_completion_context() {
        let line = "root_type My.Namespace.T";
        let pos = |character| Position { line: 0, character };

        // Cursor at the end
        let (range, partial) = get_root_type_completion_context(line, pos(24)).unwrap();
        assert_eq!(partial, "My.Namespace.T");
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 24);

        // Cursor in the middle
        let (range, partial) = get_root_type_completion_context(line, pos(16)).unwrap();
        assert_eq!(partial, "My.Nam");
        assert_eq!(range.start.character, 10);
        assert_eq!(range.end.character, 16);

        let line2 = "root_type T";
        let (range2, partial2) = get_root_type_completion_context(line2, pos(11)).unwrap();
        assert_eq!(partial2, "T");
        assert_eq!(range2.start.character, 10);
        assert_eq!(range2.end.character, 11);

        let line3 = "  root_type T";
        let (range3, partial3) = get_root_type_completion_context(line3, pos(13)).unwrap();
        assert_eq!(partial3, "T");
        assert_eq!(range3.start.character, 12);
        assert_eq!(range3.end.character, 13);
    }
}
