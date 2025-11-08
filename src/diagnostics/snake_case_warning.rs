use std::{fs, path::PathBuf, str::FromStr};

use crate::diagnostics::ErrorDiagnosticHandler;
use crate::{diagnostics::codes::DiagnosticCode, utils::as_pos_idx};
use heck::ToSnakeCase;
use log::error;
use regex::Regex;
use tower_lsp_server::lsp_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Uri,
};

// Regex to capture snake_case warnings:
// <1file>:<2line>: <3col>: warning: field names should be lowercase snake_case, got: <4name>
static SNAKE_CASE_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(
        r"^(.+?):(\d+): (\d+): warning: field names should be lowercase snake_case, got: (.+)$",
    )
    .unwrap()
});

pub struct SnakeCaseWarningHandler;

impl ErrorDiagnosticHandler for SnakeCaseWarningHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(PathBuf, Diagnostic)> {
        let captures = SNAKE_CASE_RE.captures(line)?;
        let file_path = captures[1].trim();
        let Ok(file_path) = fs::canonicalize(file_path) else {
            error!("failed to canonicalize file: {file_path}");
            return None;
        };

        let line_num: u32 = captures[2].parse().unwrap_or(1u32).saturating_sub(1);
        let col_num: u32 = captures[3].parse().unwrap_or(1);
        let name = captures[4].trim();
        let name_length = as_pos_idx(name.chars().count());

        let replacement = name.to_snake_case();
        let message = format!("field `{name}` should be in snake_case e.g. `{replacement}`");

        let range = Range {
            start: Position::new(line_num, col_num.saturating_sub(name_length)),
            end: Position::new(line_num, col_num),
        };

        Some((
            file_path,
            Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(
                    DiagnosticCode::NonSnakeCase.as_str().to_string(),
                )),
                code_description: Uri::from_str("https://flatbuffers.dev/schema/#style-guide")
                    .map(|u| CodeDescription { href: u })
                    .ok(),

                message,
                data: Some(
                    serde_json::json!({ "original_name": name, "replacement_name": replacement }),
                ),
                ..Default::default()
            },
        ))
    }
}
