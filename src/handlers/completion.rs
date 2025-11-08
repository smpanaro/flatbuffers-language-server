use crate::analysis::WorkspaceSnapshot;
use crate::ext::duration::DurationFormat;
use crate::symbol_table::{Symbol, SymbolKind};
use crate::utils::as_pos_idx;
use crate::utils::paths::uri_to_path_buf;
use log::debug;
use regex::Regex;
use ropey::Rope;
use std::iter::once;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Instant;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionParams,
    CompletionResponse, CompletionTextEdit, Documentation, MarkupContent, MarkupKind, Position,
    Range, TextEdit,
};

static FIELD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\w+)\s*:\s*([\w\.]*)").unwrap());
static ROOT_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*root_type\s+([\w\.]*)").unwrap());

#[allow(clippy::too_many_lines)]
fn handle_attribute_completion(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    position: Position,
    line: &str,
) -> Option<CompletionResponse> {
    if let Some(start_paren) = line[..position.character as usize].rfind('(') {
        // Ignore if inside a comment
        if let Some(comment_start) = line.find("//") {
            if start_paren > comment_start {
                return None;
            }
        }

        let trigger_text = &line[start_paren + 1..position.character as usize];
        let last_word = trigger_text
            .split(|c: char| c.is_whitespace() || c == ',' || c == ':')
            .next_back()
            .unwrap_or("");

        let mut items = Vec::new();
        let common_attributes = ["deprecated", "required", "key", "id"];
        let trigger_char = line[start_paren..position.character as usize]
            .chars()
            .last()
            .unwrap_or('\0');
        let attribute_prefix = if trigger_char == ',' { " " } else { "" };

        // ID completion
        if "id".starts_with(last_word) {
            if let Some(table_symbol) = snapshot.find_enclosing_table(path, position) {
                if let SymbolKind::Table(table) = &table_symbol.kind {
                    let mut max_id = -1;
                    let mut style_with_space = true;

                    for field in &table.fields {
                        if let SymbolKind::Field(f) = &field.kind {
                            if f.has_id {
                                if f.id > max_id {
                                    max_id = f.id;
                                }
                                // Check styling
                                if let Some(line) = snapshot
                                    .documents
                                    .get(path)
                                    .unwrap()
                                    .lines()
                                    .nth(field.info.location.range.start.line as usize)
                                {
                                    let line_str = line.to_string();
                                    if let Some(id_attr) = line_str.find("id:") {
                                        if line_str.chars().nth(id_attr + 3).unwrap_or(' ') != ' ' {
                                            style_with_space = false;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let has_id_attribute = line.contains("id:");
                    if !has_id_attribute {
                        let next_id = max_id + 1;
                        let label = if style_with_space {
                            format!("id: {next_id}")
                        } else {
                            format!("id:{next_id}")
                        };

                        let range = Range {
                            start: Position {
                                line: position.line,
                                character: (position.character - as_pos_idx(last_word.len())),
                            },
                            end: position,
                        };

                        let insert_text = Some(attribute_prefix.to_string() + &label);
                        items.push(CompletionItem {
                            label,
                            insert_text,
                            kind: Some(CompletionItemKind::PROPERTY),
                            detail: Some("next available id".to_string()),
                            documentation: Some(Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: "The next available field id for this table. IDs must be contiguous and start at 0.".to_string(),
                            })),
                            sort_text: Some("00".to_string()),
                            text_edit: Some(CompletionTextEdit::Edit(
                                TextEdit {
                                    range,
                                    new_text: if style_with_space {
                                        format!("id: {next_id}")
                                    } else {
                                        format!("id:{next_id}")
                                    },
                                },
                            )),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // Other attributes
        let attribute_list = &line[start_paren..];
        let value_attributes = ["force_align", "nested_flatbuffer", "hash"]; // attributes that require a value
        for entry in snapshot.symbols.builtin_attributes.iter() {
            let (name, attr) = entry;

            if attribute_list.contains(name) {
                continue;
            }
            if let Some(restricted_to_types) = &attr.restricted_to_types {
                if !restricted_to_types.iter().any(|t| line.contains(t)) {
                    continue;
                }
            }

            if name.starts_with(last_word) {
                let sort_text = if common_attributes.contains(&name.as_str()) {
                    format!("0_{name}")
                } else {
                    format!("1_{name}")
                };
                let insert_suffix = if value_attributes.contains(&name.as_str()) {
                    ":"
                } else {
                    ""
                };
                items.push(CompletionItem {
                    label: name.clone(),
                    insert_text: Some(attribute_prefix.to_string() + name + insert_suffix),
                    kind: Some(CompletionItemKind::PROPERTY),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: attr.doc.clone(),
                    })),
                    sort_text: Some(sort_text),
                    ..Default::default()
                });
            }
        }
        return Some(CompletionResponse::Array(items));
    }
    None
}

#[allow(clippy::too_many_lines)]
fn handle_field_type_completion(
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
    let captures = FIELD_RE.captures(line)?;
    let field_name = captures.get(1).map_or("", |m| m.as_str());

    let mut items = Vec::new();

    // Build a map to detect name collisions
    let mut name_collisions: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in &snapshot.symbols.global {
        let symbol = entry.1;
        if let SymbolKind::Field(_) = &symbol.kind {
            continue;
        }
        *name_collisions.entry(symbol.info.name.clone()).or_default() += 1;
    }

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
            let has_collision = name_collisions.get(base_name).is_some_and(|&c| c > 1);

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

fn handle_root_type_completion(
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

fn handle_keyword_completion(
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

fn should_suppress_completion(doc: &Rope, position: Position) -> bool {
    if (position.line as usize) >= doc.len_lines() {
        return false;
    }
    let line = doc.line(position.line as usize);

    // Only suppress on a line that is empty up to the cursor
    if !line
        .slice(0..position.character as usize)
        .to_string()
        .trim()
        .is_empty()
    {
        return false;
    }

    let mut open_braces = 0;
    let mut close_braces = 0;

    // Count braces on previous lines
    for i in 0..position.line {
        // This is safe because we checked position.line < doc.len_lines()
        let prev_line = doc.line(i as usize);
        let line_str = prev_line.to_string();
        // A bit naive, doesn't account for braces in comments or strings.
        // But probably good enough for now.
        open_braces += line_str.matches('{').count();
        close_braces += line_str.matches('}').count();
    }

    // If we have more open than close braces, we are inside a block.
    open_braces > close_braces
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
            let partial_text = partial_match.as_str().to_string();
            (range, partial_text)
        })
    })
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

pub fn handle_completion(
    snapshot: &WorkspaceSnapshot<'_>,
    params: &CompletionParams,
) -> Option<CompletionResponse> {
    let start = Instant::now();
    let position = params.text_document_position.position;

    let path = uri_to_path_buf(&params.text_document_position.text_document.uri).ok()?;

    let doc = snapshot.documents.get(&path)?;
    let line = doc
        .lines()
        .nth(position.line as usize)
        .map(|s| s.to_string())?;

    if should_suppress_completion(&doc, position) {
        return None;
    }

    let response = if let Some(response) =
        handle_attribute_completion(snapshot, &path, position, &line)
    {
        Some(response)
    } else if let Some(response) = handle_root_type_completion(snapshot, &path, &line, position) {
        Some(response)
    } else if let Some(response) = handle_field_type_completion(snapshot, &path, &line, position) {
        Some(response)
    } else {
        handle_keyword_completion(snapshot, &line)
    };

    let elapsed = start.elapsed();
    debug!(
        "completion in {}: {} L{}C{} -> {} items",
        elapsed.log_str(),
        path.display(),
        position.line + 1,
        position.character + 1,
        response.as_ref().map_or(0, |r| match r {
            CompletionResponse::Array(ref a) => a.len(),
            CompletionResponse::List(ref l) => l.items.len(),
        })
    );

    response
}

fn generate_include_text_edit(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    symbol: &Symbol,
) -> (Option<Vec<TextEdit>>, Option<String>) {
    if symbol.info.location.path != *path {
        let is_already_included = snapshot
            .dependencies
            .includes
            .get(path)
            .is_some_and(|includes| includes.iter().any(|p| p == &symbol.info.location.path));

        if !is_already_included {
            if let Some(relative_path) =
                pathdiff::diff_paths(&symbol.info.location.path, path.parent().unwrap())
            {
                if let Some(doc) = snapshot.documents.get(path) {
                    let edit = generate_include_edit(&doc, &relative_path.to_string_lossy());
                    let preview = edit.new_text.trim().strip_suffix(";").map(String::from);
                    return (Some(vec![edit]), preview);
                }
            }
        }
    }
    (None, None)
}

fn generate_include_edit(doc: &Rope, relative_path: &str) -> TextEdit {
    let last_include_line = doc
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_string().trim().starts_with("include "))
        .last()
        .map(|(i, _)| as_pos_idx(i));

    let include_insert_line = last_include_line.map_or(0, |line| line + 1);
    let include_insert_pos = Position::new(include_insert_line, 0);

    let mut new_text = format!("include \"{relative_path}\";\n");

    if let Some(line_after) = doc.lines().nth(include_insert_line as usize) {
        if !line_after.to_string().trim().is_empty() {
            new_text.push('\n');
        }
    }

    TextEdit {
        range: Range::new(include_insert_pos, include_insert_pos),
        new_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_get_field_type_completion_context() {
        let line = "  field: My.Namespace.T";
        let pos = |character| Position { line: 0, character };

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

        let line2 = "field:T";
        let (range2, partial2) = get_field_type_completion_context(line2, pos(7)).unwrap();
        assert_eq!(partial2, "T");
        assert_eq!(range2.start.character, 6);
        assert_eq!(range2.end.character, 7);
    }

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

    #[test]
    fn test_generate_include_edit_no_includes_no_namespace() {
        let doc = Rope::from_str("table MyTable {}");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 0);
    }

    #[test]
    fn test_generate_include_edit_no_includes_with_namespace() {
        let doc = Rope::from_str("namespace MyNamespace;\n\ntable MyTable {}");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 0);
    }

    #[test]
    fn test_generate_include_edit_with_includes() {
        let doc = Rope::from_str("include \"b.fbs\";\n\nnamespace MyNamespace;");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n");
        assert_eq!(edit.range.start.line, 1);
    }

    #[test]
    fn test_generate_include_edit_with_includes_no_gap() {
        let doc = Rope::from_str("include \"b.fbs\";\nnamespace MyNamespace;");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 1);
    }
}
