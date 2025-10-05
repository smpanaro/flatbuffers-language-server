use crate::analysis::find_enclosing_table;
use crate::ext::duration::DurationFormat;
use crate::server::Backend;
use crate::symbol_table::SymbolKind;
use log::debug;
use once_cell::sync::Lazy;
use regex::Regex;
use ropey::Rope;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Documentation,
    MarkupContent, MarkupKind, Position, Range, TextEdit, Url,
};

static FIELD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\w+)\s*:\s*(\w*)").unwrap());

fn handle_attribute_completion(
    backend: &Backend,
    uri: &Url,
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
            .last()
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
            if let Some(table_symbol) = find_enclosing_table(&backend.workspace, uri, position) {
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
                                if let Some(line) = backend
                                    .document_map
                                    .get(uri.as_str())
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

                    let has_id_attribute = line.find("id:") != None;
                    if !has_id_attribute {
                        let next_id = max_id + 1;
                        let label = if style_with_space {
                            format!("id: {}", next_id)
                        } else {
                            format!("id:{}", next_id)
                        };

                        let range = Range {
                            start: Position {
                                line: position.line,
                                character: (position.character - last_word.len() as u32),
                            },
                            end: position,
                        };

                        let insert_text = Some(attribute_prefix.to_string() + &label);
                        items.push(CompletionItem {
                            label,
                            insert_text: insert_text,
                            kind: Some(CompletionItemKind::PROPERTY),
                            detail: Some("next available id".to_string()),
                            documentation: Some(Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: "The next available field id for this table. IDs must be contiguous and start at 0.".to_string(),
                            })),
                            sort_text: Some("00".to_string()),
                            text_edit: Some(tower_lsp::lsp_types::CompletionTextEdit::Edit(
                                TextEdit {
                                    range,
                                    new_text: if style_with_space {
                                        format!("id: {}", next_id)
                                    } else {
                                        format!("id:{}", next_id)
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
        for entry in backend.workspace.builtin_attributes.iter() {
            let (name, attr) = (entry.key(), entry.value());

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
                    format!("0_{}", name)
                } else {
                    format!("1_{}", name)
                };
                let insert_suffix = if value_attributes.contains(&name.as_str()) {
                    ":"
                } else {
                    ""
                };
                items.push(CompletionItem {
                    label: name.clone(),
                    insert_text: Some(attribute_prefix.to_string() + &name + insert_suffix),
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

fn handle_field_type_completion(backend: &Backend, line: &str) -> Option<CompletionResponse> {
    let curr_char = line.chars().last();
    let prev_char = line.chars().nth(line.chars().count().saturating_sub(2));
    if curr_char == Some(' ') && prev_char != Some(':') {
        return None;
    }

    let Some(captures) = FIELD_RE.captures(line) else {
        return None;
    };
    let field_name = captures.get(1).map_or("", |m| m.as_str());
    let partial_text = captures.get(2).map_or("", |m| m.as_str());

    let mut items = Vec::new();

    // Build a map to detect name collisions
    let mut name_collisions: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in backend.workspace.symbols.iter() {
        let symbol = entry.value();
        if let SymbolKind::Field(_) = &symbol.kind {
            continue;
        }
        *name_collisions.entry(symbol.info.name.clone()).or_default() += 1;
    }

    // User-defined symbols
    for entry in backend.workspace.symbols.iter() {
        let symbol = entry.value();
        let kind: CompletionItemKind = (&symbol.kind).into();
        if kind == CompletionItemKind::FIELD {
            continue;
        }

        let base_name = &symbol.info.name;
        let qualified_name = if symbol.info.namespace.is_empty() {
            base_name.clone()
        } else {
            format!("{}.{}", symbol.info.namespace.join("."), base_name)
        };

        let (is_match, sort_text) = field_sort_text(
            field_name,
            partial_text,
            &symbol.info.name,
            &symbol.info.namespace,
            false,
        );

        if is_match {
            let has_collision = name_collisions.get(base_name).map_or(false, |&c| c > 1);

            let detail = if symbol.info.namespace.is_empty() {
                symbol.type_name().to_string()
            } else {
                format!(
                    "{} in {}",
                    symbol.type_name(),
                    symbol.info.namespace.join(".")
                )
            };

            let insert_text = if has_collision {
                Some(qualified_name.clone())
            } else {
                None // Let LSP client use the label
            };

            items.push(CompletionItem {
                label: base_name.clone(),
                insert_text,
                filter_text: Some(qualified_name.clone()),
                sort_text: Some(sort_text),
                kind: Some(kind),
                detail: Some(detail),
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
        let (is_match, sort_text) =
            field_sort_text(field_name, partial_text, &symbol.info.name, &vec![], true);

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

    Some(CompletionResponse::Array(items))
}

/// Determines if a symbol is a relevant completion and calculates its sort order.
///
/// The sorting logic prioritizes matches in the following order:
/// 1.  **Exact Namespace and Type Prefix Match**: `my.thing: My.Th` -> `My.Thing`
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
    symbol_name: &str,
    symbol_namespace: &[String],
    is_builtin: bool,
) -> (bool, String) {
    let qualified_name = if symbol_namespace.is_empty() {
        symbol_name.to_string()
    } else {
        format!("{}.{}", symbol_namespace.join("."), symbol_name)
    };

    let (is_match, sort_prefix) = if let Some((ns_part, type_part)) = partial_text.rsplit_once('.')
    {
        // Case 1: User is typing a qualified name (e.g., "My.Api.")
        let is_ns_match = symbol_namespace.join(".").starts_with(ns_part);
        let is_type_match = symbol_name.starts_with(type_part);
        let is_a_match = is_ns_match && is_type_match;
        (is_a_match, if is_a_match { "0" } else { "4" })
    } else {
        // Case 2: User is typing a type name or namespace directly
        let is_type_match = symbol_name
            .to_lowercase()
            .contains(&partial_text.to_lowercase());
        let is_type_prefix_match = symbol_name
            .to_lowercase()
            .starts_with(&partial_text.to_lowercase());
        let is_ns_match = symbol_namespace
            .iter()
            .any(|ns| ns.starts_with(partial_text));
        let field_name_contains_type = field_name
            .to_lowercase()
            .contains(&symbol_name.to_lowercase());

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

    let sort_text = format!("{}_{}", sort_prefix, qualified_name);
    (is_match, sort_text)
}

fn handle_root_type_completion(backend: &Backend, line: &str) -> Option<CompletionResponse> {
    let trimmed_line = line.trim();
    if !trimmed_line.starts_with("root_type") {
        return None;
    }

    let partial_text = trimmed_line.strip_prefix("root_type").unwrap_or("").trim();

    let mut items = Vec::new();
    for entry in backend.workspace.symbols.iter() {
        let symbol = entry.value();
        if let SymbolKind::Table(_) = &symbol.kind {
            let base_name = &symbol.info.name;
            let qualified_name = if symbol.info.namespace.is_empty() {
                base_name.clone()
            } else {
                format!("{}.{}", symbol.info.namespace.join("."), base_name)
            };

            if qualified_name.starts_with(partial_text) {
                items.push(CompletionItem {
                    label: base_name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
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
    }

    Some(CompletionResponse::Array(items))
}

fn handle_keyword_completion(backend: &Backend, line: &str) -> Option<CompletionResponse> {
    let partial_keyword = line.trim();
    let items: Vec<CompletionItem> = backend
        .workspace
        .keywords
        .iter()
        .filter(|item| item.key().starts_with(partial_keyword))
        .map(|item| CompletionItem {
            label: item.key().clone(),
            kind: Some(CompletionItemKind::KEYWORD),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: item.value().clone(),
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
    let Some(line) = doc
        .lines()
        .nth(position.line as usize)
        .map(|s| s.to_string())
    else {
        return Ok(None);
    };

    if should_suppress_completion(&*doc, position) {
        return Ok(None);
    }

    let response =
        if let Some(response) = handle_attribute_completion(backend, &uri, position, &line) {
            Some(response)
        } else if let Some(response) = handle_root_type_completion(backend, &line) {
            Some(response)
        } else if let Some(response) = handle_field_type_completion(backend, &line) {
            Some(response)
        } else {
            handle_keyword_completion(backend, &line)
        };

    let elapsed = start.elapsed();
    debug!(
        "completion in {}: {} L{}C{} -> {} items",
        elapsed.log_str(),
        &uri.path(),
        position.line + 1,
        position.character + 1,
        response.as_ref().map_or(0, |r| match r {
            CompletionResponse::Array(ref a) => a.len(),
            CompletionResponse::List(ref l) => l.items.len(),
        })
    );

    Ok(response)
}
