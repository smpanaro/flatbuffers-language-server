use crate::server::Backend;
use regex::Regex;
use std::collections::HashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    DiagnosticSeverity, NumberOrString, Position, Range, TextEdit, WorkspaceEdit,
};

pub async fn handle_code_action(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    let uri = params.text_document.uri;
    let doc = backend.document_map.get(&uri.to_string()).unwrap();

    let mut code_actions = Vec::new();

    for diagnostic in params.context.diagnostics {
        if diagnostic.code == Some(NumberOrString::String("expecting-token".to_string()))
            && diagnostic.severity == Some(DiagnosticSeverity::ERROR)
        {
            if let Some(data) = &diagnostic.data {
                if let Some(expected) = data.get("expected").map(|v| v.as_str()).flatten() {
                    let end_of_line = data
                        .get("eol")
                        .map(|v| v.as_bool())
                        .flatten()
                        .unwrap_or(false);
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

        let unused_include_re = Regex::new(r"unused include: (.+)").unwrap();
        if let Some(_) = unused_include_re.captures(&diagnostic.message) {
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

        let undefined_type_re =
            Regex::new(r"type referenced but not defined \(check namespace\): (\w+)").unwrap();
        let Some(captures) = undefined_type_re.captures(&diagnostic.message) else {
            continue;
        };
        let type_name = captures.get(1).unwrap().as_str();

        for symbol_entry in backend.workspace.symbols.iter() {
            if symbol_entry.value().info.name == type_name {
                let symbol = symbol_entry.value();
                let Ok(symbol_path) = symbol.info.location.uri.to_file_path() else {
                    continue;
                };
                let Ok(current_path) = uri.to_file_path() else {
                    continue;
                };
                let Some(current_dir) = current_path.parent() else {
                    continue;
                };
                let Some(relative_path) = pathdiff::diff_paths(&symbol_path, &current_dir) else {
                    continue;
                };

                let last_include_line = doc
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| line.to_string().starts_with("include "))
                    .last()
                    .map(|(i, _)| i as u32);
                let insert_line = last_include_line.map_or(0, |line| line + 1);

                let text_edit = TextEdit {
                    range: Range::new(Position::new(insert_line, 0), Position::new(insert_line, 0)),
                    new_text: format!("include \"{}\";\n", relative_path.to_str().unwrap()),
                };

                let mut changes = HashMap::new();
                changes.insert(uri.clone(), vec![text_edit]);

                let code_action = CodeAction {
                    title: format!(
                        "Import `{}` from `{}`",
                        type_name,
                        relative_path.to_str().unwrap()
                    ),
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

    Ok(Some(code_actions))
}
