use crate::analysis::WorkspaceSnapshot;
use crate::symbol_table::SymbolKind;
use crate::utils::as_pos_idx;
use std::{cmp::max, path::PathBuf};
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit, Documentation,
    MarkupContent, MarkupKind, Position, Range, TextEdit,
};

#[allow(clippy::too_many_lines)]
pub fn handle_attribute_completion(
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
                            if let Some(id) = f.id {
                                max_id = max(max_id, id);
                            }

                            // Check styling
                            if let Some(line) = snapshot.documents.get(path).and_then(|doc| {
                                doc.lines()
                                    .nth(field.info.location.range.start.line as usize)
                                    .map(|line| line.to_string())
                            }) {
                                if let Some(id_attr) = line.find("id:") {
                                    if line.chars().nth(id_attr + 3).unwrap_or(' ') != ' ' {
                                        style_with_space = false;
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
