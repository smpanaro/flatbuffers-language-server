use crate::utils::as_pos_idx;
use crate::{analysis::WorkspaceSnapshot, handlers::completion::util::generate_include_text_edit};
use regex::Regex;
use std::iter::once;
use std::path::PathBuf;
use std::sync::LazyLock;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionResponse,
    CompletionTextEdit, Documentation, MarkupContent, MarkupKind, Position, Range, TextEdit,
};

static FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(\w+)\s*:\s*\[?\s*([\w\.]*)").expect("field type regex failed to compile")
});

#[allow(clippy::too_many_lines)]
pub fn handle_field_type_completion(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    line: &str,
    position: Position,
) -> Option<CompletionResponse> {
    let curr_char = line.chars().last();
    let prev_char = line.chars().nth(line.chars().count().saturating_sub(2));
    if curr_char == Some(' ') && prev_char != Some(':') {
        return None;
    }
    let (range, partial_text) = get_field_type_completion_context(line, position)?;
    if partial_text.trim_start().contains(char::is_whitespace) {
        // Cannot have spaces within a type.
        return None;
    }
    let captures = FIELD_RE.captures(line)?;
    let field_name = captures.get(1).map_or("", |m| m.as_str());

    let mut items = Vec::new();

    let collisions = snapshot.symbols.collisions();

    // User-defined symbols
    for entry in &snapshot.symbols.global {
        let symbol = entry.1;
        let kind: CompletionItemKind = (&symbol.kind).into();
        if kind == CompletionItemKind::FIELD {
            continue;
        }

        let base_name = &symbol.info.name;
        let qualified_name = symbol.info.qualified_name();

        let (is_match, sort_text) = field_sort_text(
            field_name,
            &partial_text,
            Some(&symbol.info.name),
            &symbol
                .info
                .namespace
                .iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>(),
            false,
        );

        if is_match {
            let has_collision = collisions.contains_key(base_name);

            let detail = symbol.info.namespace_str().map_or_else(
                || symbol.type_name().to_string(),
                |ns| format!("{} in {}", symbol.type_name(), ns),
            );

            let use_qualified = partial_text.contains('.') || has_collision;
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
                filter_text: Some(qualified_name.clone()),
                sort_text: Some(sort_text),
                kind: Some(kind),
                detail: Some(detail),
                label_details: Some(CompletionItemLabelDetails {
                    detail: None, // for function signatures or type annotations, neither of which are relevant for us.
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

    // Built-in symbols
    for item in snapshot.symbols.builtins.iter() {
        let (name, symbol) = item;
        let (is_match, sort_text) = field_sort_text(
            field_name,
            &partial_text,
            Some(&symbol.info.name),
            &[],
            true,
        );

        if is_match {
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
    }

    // Namespaces
    for ns in snapshot.symbols.namespaces() {
        let (is_match, sort_text) = field_sort_text(
            field_name,
            &partial_text,
            None,
            &ns.split('.').collect::<Vec<_>>(),
            false,
        );

        if is_match {
            items.push(CompletionItem {
                label: ns.clone(),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: ns.clone(),
                })),
                sort_text: Some(sort_text),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some("namespace".to_string()),
                ..Default::default()
            });
        }
    }

    Some(CompletionResponse::Array(items))
}

/// Determines if a symbol is a relevant completion and calculates its sort order.
///
/// The sorting logic prioritizes matches in the following order:
/// 1.  **Exact Namespace and Type Prefix Match**: `my_thing: My.Th` -> `My.Thing`
/// 2.  **Type Name in Field Name**: `my_widget: ` -> `Widget`
/// 3.  **Type Prefix Match**: `my_field: Wi` -> `Widget`
/// 4.  **Substring Match**: `my_field: dget` -> `Widget`
/// 5.  **Namespace Prefix Match**: `my_field: My` -> `My.Thing`
///
/// ## Returns
/// A tuple `(is_match, sort_text)` where:
/// - `is_match`: A boolean indicating if the symbol is a candidate for completion.
/// - `sort_text`: A string used for sorting the completion item.
fn field_sort_text(
    field_name: &str,
    partial_text: &str,
    symbol_name: Option<&str>,
    symbol_namespace: &[&str],
    is_builtin: bool,
) -> (bool, String) {
    let qualified_name = symbol_namespace
        .iter()
        .map(|&s| Some(s))
        .chain(once(symbol_name))
        .flatten()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(".");

    // Don't suggest a namespace if it is complete.
    if symbol_name.is_none() && partial_text == format!("{qualified_name}.") {
        return (false, String::new());
    }

    let (is_match, sort_prefix) = if let Some((ns_part, type_part)) = partial_text.rsplit_once('.')
    {
        // Case 1: User is typing a qualified name (e.g., "My.Api.")
        let is_ns_match = symbol_namespace.join(".").starts_with(ns_part);
        let is_type_match = symbol_name.is_none_or(|sn| sn.starts_with(type_part));
        let is_a_match = is_ns_match && is_type_match;
        (is_a_match, if is_a_match { "0" } else { "4" })
    } else {
        // Case 2: User is typing a type name or namespace directly
        let is_type_match =
            symbol_name.is_some_and(|sn| sn.to_lowercase().contains(&partial_text.to_lowercase()));
        let is_type_prefix_match = symbol_name
            .is_some_and(|sn| sn.to_lowercase().starts_with(&partial_text.to_lowercase()));
        let is_ns_match = symbol_namespace
            .iter()
            .any(|ns| ns.starts_with(partial_text));
        let field_name_contains_type =
            symbol_name.is_some_and(|sn| field_name.to_lowercase().contains(&sn.to_lowercase()));

        if is_type_match {
            if field_name_contains_type {
                (true, "0") // Perfect match: `my_widget` -> `Widget`
            } else if is_type_prefix_match {
                (true, "1") // Good match: `my_field: Wi` -> `Widget`
            } else {
                (true, "2") // Ok match: `my_field: u` -> `double`
            }
        } else if is_ns_match {
            (true, "3") // Namespace match
        } else if !is_builtin {
            (false, "4") // custom types before builtins
        } else {
            (false, "5")
        }
    };

    let sort_text = format!("{sort_prefix}_{qualified_name}");
    (is_match, sort_text)
}

fn get_field_type_completion_context(line: &str, position: Position) -> Option<(Range, String)> {
    let line_upto_cursor = &line[..position.character as usize];
    FIELD_RE.captures(line_upto_cursor).and_then(|captures| {
        captures.get(2).map(|partial_match| {
            let start_char = as_pos_idx(line_upto_cursor[..partial_match.start()].chars().count());
            let range = Range {
                start: Position {
                    line: position.line,
                    character: start_char,
                },
                end: position,
            };
            let partial_text = line_upto_cursor[partial_match.start()..].to_string();
            (range, partial_text)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_field_type_completion_context() {
        let pos = |character| Position { line: 0, character };

        {
            let line = "  field: My.Namespace.T";

            // Cursor at the end
            let (range, partial) = get_field_type_completion_context(line, pos(23)).unwrap();
            assert_eq!(partial, "My.Namespace.T");
            assert_eq!(range.start.character, 9);
            assert_eq!(range.end.character, 23);

            // Cursor in the middle
            let (range, partial) = get_field_type_completion_context(line, pos(15)).unwrap();
            assert_eq!(partial, "My.Nam");
            assert_eq!(range.start.character, 9);
            assert_eq!(range.end.character, 15);
        }

        {
            let line = "field:T";
            let (range, partial) = get_field_type_completion_context(line, pos(7)).unwrap();
            assert_eq!(partial, "T");
            assert_eq!(range.start.character, 6);
            assert_eq!(range.end.character, 7);
        }

        {
            let line = "  field: [T";
            let (range, partial) = get_field_type_completion_context(line, pos(11)).unwrap();
            assert_eq!(partial, "T");
            assert_eq!(range.start.character, 10);
            assert_eq!(range.end.character, 11);
        }

        {
            let line = "  field: int i";
            let (range, partial) = get_field_type_completion_context(line, pos(14)).unwrap();
            assert_eq!(partial, "int i");
            assert_eq!(range.start.character, 9);
            assert_eq!(range.end.character, 14);
        }
    }

    #[test]
    fn test_field_sort_text() {
        assert!(field_sort_text("bean", "pastries.", Some("Bean"), &["pastries"], false).0);
        assert!(
            field_sort_text(
                "bean",
                "pastri",
                Some("Bean"),
                &["pastries", "vanilla"],
                false
            )
            .0
        );
        assert!(field_sort_text("bean", "Be", Some("Bean"), &["pastries"], false).0);
        assert!(
            // Helpful to see extra metadata for what was selected.
            field_sort_text("bean", "pastries", None, &["pastries"], false).0
        );
        assert!(
            // Should not insert pastries.pastries again.
            !field_sort_text("bean", "pastries.", None, &["pastries"], false).0
        );
        assert!(
            // Should suggest `one.two.three`.
            field_sort_text("bean", "one.two.", None, &["one", "two", "three"], false).0
        );
    }
}
