use crate::diagnostics::codes::DiagnosticCode;
use crate::server::Backend;
use crate::utils::paths::uri_to_path_buf;

use serde_json::Value;
use std::collections::HashMap;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, TextEdit, Uri, WorkspaceEdit,
};

/// Handles incoming code action requests from the LSP client.
///
/// This function iterates through diagnostics provided by the client and generates
/// relevant quick-fix actions based on the diagnostic code.
pub async fn handle_code_action(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;
    let mut code_actions = Vec::new();

    for diagnostic in params.context.diagnostics {
        let Some(NumberOrString::String(code_str)) = &diagnostic.code else {
            continue;
        };
        let Ok(code) = DiagnosticCode::try_from(code_str.clone()) else {
            continue;
        };

        match code {
            DiagnosticCode::ExpectingToken => {
                if diagnostic.severity != Some(DiagnosticSeverity::ERROR) {
                    continue;
                }
                if let Some(data) = &diagnostic.data {
                    if let Some(expected) = data.get("expected").and_then(|v| v.as_str()) {
                        let end_of_line =
                            data.get("eol").and_then(|v| v.as_bool()).unwrap_or(false);
                        if expected != "identifier" {
                            let start = diagnostic.range.start;
                            let insertion_pos = Position::new(
                                start.line,
                                // Diagnostic character is truncated to the end of the line,
                                // regardless of sent diagnostic.
                                start.character + if end_of_line { 1 } else { 0 },
                            );
                            let text_edit = TextEdit {
                                range: Range::new(insertion_pos, insertion_pos),
                                new_text: expected.to_string(),
                            };
                            let mut changes = HashMap::new();
                            changes.insert(uri.clone(), vec![text_edit]);
                            let code_action = CodeAction {
                                title: format!("Add missing `{}`", expected),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diagnostic.clone()]),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(changes),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            };
                            code_actions.push(CodeActionOrCommand::CodeAction(code_action));
                        }
                    }
                }
            }
            DiagnosticCode::NonSnakeCase => {
                if let Some(data) = &diagnostic.data {
                    if let (
                        Some(Value::String(original_name)),
                        Some(Value::String(replacement_name)),
                    ) = (data.get("original_name"), data.get("replacement_name"))
                    {
                        let text_edit = TextEdit {
                            range: diagnostic.range,
                            new_text: replacement_name.clone(),
                        };
                        let mut changes = HashMap::new();
                        changes.insert(uri.clone(), vec![text_edit]);
                        let edit = WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        };
                        let code_action = CodeAction {
                            title: format!("Rename `{}` to `{}`", original_name, replacement_name),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            edit: Some(edit),
                            is_preferred: Some(true),
                            ..Default::default()
                        };
                        code_actions.push(CodeActionOrCommand::CodeAction(code_action));
                    }
                }
            }
            DiagnosticCode::UnusedInclude => {
                let range = diagnostic.range;
                let text_edit = TextEdit {
                    range: Range {
                        start: range.start,
                        end: Position {
                            line: range.end.line + 1,
                            character: 0,
                        },
                    },
                    new_text: "".to_string(),
                };
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), vec![text_edit]);
                let code_action = CodeAction {
                    title: "Remove unused include".to_string(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diagnostic.clone()]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                code_actions.push(CodeActionOrCommand::CodeAction(code_action));
            }
            DiagnosticCode::UndefinedType => {
                code_actions.extend(generate_undefined_type_code_actions(
                    backend,
                    &uri,
                    &diagnostic,
                ));
            }
        }
    }
    Ok(Some(code_actions))
}

/// Creates a CodeActionOrCommand representing a quick fix.
fn create_quickfix(
    uri: &Uri,
    diagnostic: &Diagnostic,
    title: String,
    edits: Vec<TextEdit>,
) -> CodeActionOrCommand {
    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);
    let edit = WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    };

    let code_action = CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit: Some(edit),
        ..Default::default()
    };

    CodeActionOrCommand::CodeAction(code_action)
}

/// Generates a list of code actions for an "UndefinedType" diagnostic.
///
/// This function searches the workspace for symbols that match the undefined type
/// and suggests actions such as importing the symbol, qualifying the type name,
/// or setting the file's namespace.
fn generate_undefined_type_code_actions(
    backend: &Backend,
    uri: &Uri,
    diagnostic: &Diagnostic,
) -> Vec<CodeActionOrCommand> {
    let Some(type_name) = diagnostic
        .data
        .as_ref()
        .and_then(|d| d.get("type_name"))
        .and_then(|v| v.as_str())
    else {
        return vec![];
    };

    let Ok(current_path) = uri_to_path_buf(uri) else {
        return vec![];
    };

    let Some(current_dir) = current_path.parent() else {
        return vec![];
    };

    let Some(doc) = backend.document_map.get(&current_path) else {
        return vec![];
    };

    let matching_symbols: Vec<_> = backend
        .workspace
        .symbols
        .iter()
        .filter(|s| {
            s.value().info.qualified_name() == type_name || s.value().info.name == type_name
        })
        .map(|s| s.value().clone())
        .collect();

    if matching_symbols.is_empty() {
        return vec![];
    }

    let file_namespace: Option<Vec<String>> = doc.lines().find_map(|line| {
        line.to_string()
            .trim()
            .strip_prefix("namespace ")
            .and_then(|ns| ns.strip_suffix(';'))
            .map(|ns| ns.trim().split('.').map(|s| s.to_string()).collect())
    });

    let last_include_line = doc
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_string().trim().starts_with("include "))
        .last()
        .map(|(i, _)| i as u32);
    let include_insert_line = last_include_line.map_or(0, |line| line + 1);
    let include_insert_pos = Position::new(include_insert_line, 0);

    let mut code_actions = Vec::new();

    for symbol in matching_symbols {
        let symbol_path = symbol.info.location.path.to_path_buf();
        let Some(relative_path) = pathdiff::diff_paths(&symbol_path, current_dir) else {
            continue;
        };
        let relative_path_str = relative_path.to_str().unwrap_or_default();

        let is_already_included =
            backend
                .workspace
                .file_includes
                .get(&current_path)
                .map_or(false, |includes| {
                    includes
                        .iter()
                        .any(|include_path| include_path == &symbol_path)
                });

        let has_existing_includes = last_include_line.is_some();
        let include_line = format!("include \"{}\";\n", relative_path_str);
        let include_edit = if !is_already_included {
            Some(TextEdit {
                range: Range::new(include_insert_pos, include_insert_pos),
                new_text: if !has_existing_includes {
                    // Add a gap after the first include.
                    format!("{}\n", include_line.clone())
                } else {
                    include_line.clone()
                },
            })
        } else {
            None
        };

        // type_name is the token from the file, as parsed.
        // We can assume its namespace based on the file's namespace.
        let implicit_type_name = file_namespace
            .as_ref()
            .map(|ns| format!("{}.{}", ns.join("."), type_name))
            .unwrap_or_default();
        let is_qualified_match = symbol.info.qualified_name() == type_name
            || symbol.info.qualified_name() == implicit_type_name;

        if is_qualified_match {
            // Case: The type is already fully qualified (e.g., `MyNamespace.MyTable`).
            // It just needs an import.
            if let Some(edit) = include_edit {
                let title = format!("Import `{}` from `{}`", symbol.info.name, relative_path_str);
                code_actions.push(create_quickfix(uri, diagnostic, title, vec![edit]));
            }
        } else {
            // Case: The type is unqualified (e.g., `MyTable`).
            match &file_namespace {
                Some(ns) if *ns == symbol.info.namespace => {
                    // TODO: This case might be redundant.
                    // File namespace matches the symbol's namespace. Just needs an import.
                    if let Some(edit) = include_edit {
                        let title =
                            format!("Import `{}` from `{}`", symbol.info.name, relative_path_str);
                        code_actions.push(create_quickfix(uri, diagnostic, title, vec![edit]));
                    }
                }
                Some(_) => {
                    // File namespace exists but is different. Only offer to qualify.
                    let import_suffix = if !is_already_included {
                        format!(" and import from `{}`", relative_path_str)
                    } else {
                        "".to_string()
                    };
                    let mut qualify_edits = include_edit.clone().into_iter().collect::<Vec<_>>();
                    qualify_edits.push(TextEdit {
                        range: diagnostic.range,
                        new_text: symbol.info.qualified_name(),
                    });
                    let title = format!(
                        "Qualify type as `{}`{}",
                        symbol.info.qualified_name(),
                        import_suffix
                    );
                    code_actions.push(create_quickfix(uri, diagnostic, title, qualify_edits));
                }
                None => {
                    // No file namespace. Offer both qualification and setting the namespace.
                    let import_suffix = if !is_already_included {
                        format!(" and import from `{}`", relative_path_str)
                    } else {
                        "".to_string()
                    };

                    // Action 1: Qualify the type.
                    let mut qualify_edits = include_edit.clone().into_iter().collect::<Vec<_>>();
                    qualify_edits.push(TextEdit {
                        range: diagnostic.range,
                        new_text: symbol.info.qualified_name(),
                    });
                    let qualify_title = format!(
                        "Qualify type as `{}`{}",
                        symbol.info.qualified_name(),
                        import_suffix
                    );
                    code_actions.push(create_quickfix(
                        uri,
                        diagnostic,
                        qualify_title,
                        qualify_edits,
                    ));

                    // Action 2: Set the file namespace.
                    if !symbol.info.namespace.is_empty() {
                        let namespace_line =
                            format!("namespace {};\n", symbol.info.namespace.join("."));
                        let new_text = if !is_already_included {
                            // Add gap between includes and namespace.
                            if has_existing_includes {
                                // Maintain # of lines between includes and next declaration.
                                format!("{}\n{}", include_line, namespace_line)
                            } else {
                                // Add a gap for between namespace and next declaration.
                                format!("{}\n{}\n", include_line, namespace_line)
                            }
                        } else if has_existing_includes {
                            // Add gap between includes and namespace.
                            // Maintain # of lines between includes and next declaration.
                            format!("\n{}", namespace_line)
                        } else {
                            // Add gap between namespace and next declaration.
                            format!("{}\n", namespace_line)
                        };

                        let namespace_edits = vec![TextEdit {
                            range: Range::new(include_insert_pos, include_insert_pos),
                            new_text,
                        }];

                        let namespace_title = format!(
                            "Set file namespace to `{}`{}",
                            symbol.info.namespace.join("."),
                            import_suffix
                        );
                        code_actions.push(create_quickfix(
                            uri,
                            diagnostic,
                            namespace_title,
                            namespace_edits,
                        ));
                    }
                }
            }
        }
    }

    code_actions
}
