use crate::analysis::find_enclosing_table;
use crate::server::Backend;
use crate::symbol_table::SymbolKind;
use log::info;
use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse, Documentation,
    MarkupContent, MarkupKind, Position, Range, TextEdit, Url,
};

static FIELD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\w+)\s*:").unwrap());

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
                                    if let Some(id_attr) = line.find("id:") {
                                        if line.chars().nth(id_attr + 3).unwrap_or(' ') != ' ' {
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
            let (name, doc) = (entry.key(), entry.value());

            if attribute_list.contains(name) {
                continue;
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
                        value: doc.clone(),
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

    Some(CompletionResponse::Array(items))
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
    let Some(line) = doc.lines().nth(position.line as usize) else {
        return Ok(None);
    };

    let response =
        if let Some(response) = handle_attribute_completion(backend, &uri, position, line) {
            Some(response)
        } else {
            handle_field_type_completion(backend, line)
        };

    let elapsed = start.elapsed();
    info!(
        "completion in {}ms: {} L{}C{} -> {} items",
        elapsed.as_millis(),
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
