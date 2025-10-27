use std::{fs, path::PathBuf};

use crate::diagnostics::ErrorDiagnosticHandler;
use crate::{diagnostics::codes::DiagnosticCode, utils::paths::path_buf_to_uri};
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json;
use tower_lsp_server::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range,
};

static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?):\s*(\d+):\s*(\d+):\s*error:\s*expecting: (.+) instead got:\s*(.+)$")
        .unwrap()
});

pub struct ExpectingTokenHandler;

impl ErrorDiagnosticHandler for ExpectingTokenHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(PathBuf, Diagnostic)> {
        if let Some(captures) = RE.captures(line) {
            let file_path = captures[1].trim();
            let Ok(file_path) = fs::canonicalize(file_path) else {
                error!("failed to canonicalize file: {}", file_path);
                return None;
            };
            let Ok(file_url) = path_buf_to_uri(&file_path) else {
                return None;
            };

            let error_line_num: u32 = captures[2].parse().unwrap_or(1u32).saturating_sub(1);
            let error_col_num: u32 = captures[3].parse().unwrap_or(1u32).saturating_sub(1);
            let expected_token = captures[4].trim().to_string();
            let unexpected_token = captures[5].trim().to_string();

            let message = if unexpected_token == "end of file" {
                format!("expected `{}`, found `end of file`", expected_token)
            } else {
                format!(
                    "expected `{}`, found `{}`",
                    expected_token, unexpected_token
                )
            };

            let line_content = content.lines().nth(error_line_num as usize).unwrap_or("");
            let cleaned_token = unexpected_token.replace('`', "");
            let adjusted_col = line_content
                .find(&cleaned_token)
                .map(|v| v as u32)
                .unwrap_or(error_col_num);

            let line_before_error = &line_content[..adjusted_col as usize];

            // This is the start position (inclusive) of where the token would be if it were inserted.
            let diagnostic_pos =
                if line_before_error.trim().is_empty() || unexpected_token == "end of file" {
                    // The error is at the start of a new statement, or at EOF. The fix belongs on a previous line.
                    let mut line = if unexpected_token == "end of file" {
                        error_line_num
                    } else {
                        error_line_num.saturating_sub(1)
                    };

                    loop {
                        let is_empty_or_comment =
                            content.lines().nth(line as usize).map_or(true, |l| {
                                let trimmed = l.trim();
                                trimmed.is_empty() || trimmed.starts_with("//")
                            });

                        if !is_empty_or_comment {
                            break;
                        }

                        if line == 0 {
                            break;
                        }
                        line -= 1;
                    }
                    let diagnostic_line_num = line;

                    let diagnostic_line_content = content
                        .lines()
                        .nth(diagnostic_line_num as usize)
                        .unwrap_or("");
                    let diagnostic_col_num = diagnostic_line_content.chars().count() as u32;
                    Position::new(diagnostic_line_num, diagnostic_col_num)
                } else {
                    // The error is on the same line as the context; the fix belongs on this line.
                    Position::new(error_line_num, adjusted_col)
                };
            // Range is [start, end).
            let range = Range::new(
                diagnostic_pos,
                Position::new(
                    diagnostic_pos.line,
                    diagnostic_pos.character + expected_token.chars().count() as u32,
                ),
            );
            let diagnostic_line_content = content
                .lines()
                .nth(diagnostic_pos.line as usize)
                .unwrap_or("");
            let is_eol = diagnostic_line_content.chars().count() as u32 == diagnostic_pos.character;

            let mut related_information = vec![];

            if unexpected_token != "end of file" {
                let line_content = content.lines().nth(error_line_num as usize).unwrap_or("");
                let cleaned_token = unexpected_token.replace('`', "");
                let adjusted_col = line_content
                    .find(&cleaned_token)
                    .map(|v| v as u32)
                    .unwrap_or(error_col_num);

                let unexpected_token_range = Range::new(
                    Position::new(error_line_num, adjusted_col),
                    Position::new(error_line_num, adjusted_col + cleaned_token.len() as u32),
                );
                related_information.push(DiagnosticRelatedInformation {
                    location: Location {
                        uri: file_url.clone(),
                        range: unexpected_token_range,
                    },
                    message: "unexpected token".to_string(),
                });
            }

            related_information.push(DiagnosticRelatedInformation {
                location: Location {
                    uri: file_url.clone(),
                    range,
                },
                message: format!("add `{}` here", expected_token),
            });

            return Some((
                file_path,
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    message,
                    related_information: Some(related_information),
                    code: Some(tower_lsp_server::lsp_types::NumberOrString::String(
                        DiagnosticCode::ExpectingToken.as_str().to_string(),
                    )),
                    data: Some(serde_json::json!({ "expected": expected_token, "eol": is_eol })),
                    ..Default::default()
                },
            ));
        }
        None
    }
}
