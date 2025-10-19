use std::{fs, path::PathBuf};

use crate::diagnostics::codes::DiagnosticCode;
use crate::diagnostics::ErrorDiagnosticHandler;
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^.+?:(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: (.+?):(\d+)(?::(\d+)-(\d+):(\d+))?)?$")
        .unwrap()
});

static UNDEFINED_TYPE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"type referenced but not defined \(check namespace\): ((?:\w+\.?)*)").unwrap()
});

pub struct UndefinedTypeHandler;

impl ErrorDiagnosticHandler for UndefinedTypeHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(PathBuf, Diagnostic)> {
        if let Some(captures) = RE.captures(line) {
            let message = captures[4].trim().to_string();
            if let Some(undefined_type_captures) = UNDEFINED_TYPE_RE.captures(&message) {
                let file_path: &str;
                let range: Range;

                // Check if the "originally at" clause with the L:C-L:C range was captured.
                if let (
                    Some(original_path),
                    Some(start_line_str),
                    Some(start_col_str),
                    Some(end_line_str),
                    Some(end_col_str),
                ) = (
                    captures.get(5),
                    captures.get(6),
                    captures.get(7),
                    captures.get(8),
                    captures.get(9),
                ) {
                    // Case 1: "originally at" with a full range exists. Use these values.
                    file_path = original_path.as_str();

                    // Convert 1-based line from regex to 0-based for LSP.
                    let start_line = start_line_str
                        .as_str()
                        .parse()
                        .unwrap_or(1u32)
                        .saturating_sub(1);
                    let start_char = start_col_str.as_str().parse().unwrap_or(0u32);
                    let end_line = end_line_str
                        .as_str()
                        .parse()
                        .unwrap_or(1u32)
                        .saturating_sub(1);
                    // The end character in LSP is exclusive.
                    let end_char = end_col_str.as_str().parse().unwrap_or(0u32);

                    range = Range {
                        start: Position {
                            line: start_line,
                            character: start_char,
                        },
                        end: Position {
                            line: end_line,
                            character: end_char,
                        },
                    };
                } else {
                    // Case 2: No detailed "originally at" range.
                    file_path = captures.get(0).unwrap().as_str().split(':').next().unwrap();
                    let line_num: u32 = captures
                        .get(1)
                        .map_or("1", |m| m.as_str())
                        .parse()
                        .unwrap_or(1u32)
                        .saturating_sub(1);
                    let col_num: u32 = captures
                        .get(2)
                        .map_or("1", |m| m.as_str())
                        .parse()
                        .unwrap_or(1u32)
                        .saturating_sub(1);

                    // Start with a broad range for the line.
                    let mut temp_range = Range {
                        start: Position {
                            line: line_num,
                            character: col_num,
                        },
                        end: Position {
                            line: line_num,
                            character: u32::MAX,
                        },
                    };

                    // Attempt to narrow the range by finding the type name in the line content.
                    if let Some(type_name) = undefined_type_captures.get(1) {
                        if let Some(line_content) = content.lines().nth(line_num as usize) {
                            if let Some(start) = line_content.find(type_name.as_str()) {
                                let end = start + type_name.as_str().len();
                                temp_range.start.character = start as u32;
                                temp_range.end.character = end as u32;
                            }
                        }
                    }
                    range = temp_range;
                }

                let Ok(file_path) = fs::canonicalize(file_path) else {
                    error!("failed to canonicalize file: {}", file_path);
                    return None;
                };

                let severity = if &captures[3] == "error" {
                    DiagnosticSeverity::ERROR
                } else {
                    DiagnosticSeverity::WARNING
                };

                let data = undefined_type_captures
                    .get(1)
                    .map(|type_name| json!({ "type_name": type_name.as_str() }));

                return Some((
                    file_path,
                    Diagnostic {
                        range,
                        severity: Some(severity),
                        code: Some(tower_lsp_server::lsp_types::NumberOrString::String(
                            DiagnosticCode::UndefinedType.as_str().to_string(),
                        )),
                        message,
                        data,
                        ..Default::default()
                    },
                ));
            }
        }
        None
    }
}
