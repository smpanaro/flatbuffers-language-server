use std::str::FromStr;

use crate::diagnostics::codes::DiagnosticCode;
use crate::diagnostics::ErrorDiagnosticHandler;
use heck::ToSnakeCase;
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use tower_lsp::lsp_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Url,
};

// Regex to capture snake_case warnings:
// <1file>:<2line>: <3col>: warning: field names should be lowercase snake_case, got: <4name>
static SNAKE_CASE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(.+?):(\d+): (\d+): warning: field names should be lowercase snake_case, got: (.+)$",
    )
    .unwrap()
});

pub struct SnakeCaseWarningHandler;

impl ErrorDiagnosticHandler for SnakeCaseWarningHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(Url, Diagnostic)> {
        let Some(captures) = SNAKE_CASE_RE.captures(line) else {
            return None;
        };
        let file_path = captures[1].trim();
        let Ok(file_uri) = Url::from_file_path(file_path) else {
            error!("failed to parse file into url: {}", file_path);
            return None;
        };

        let line_num: u32 = captures[2].parse().unwrap_or(1u32).saturating_sub(1);
        let col_num: u32 = captures[3].parse().unwrap_or(1);
        let name = captures[4].trim();
        let name_length = name.chars().count() as u32;

        let replacement = name.to_snake_case();
        let message = format!(
            "field `{}` should be in snake_case e.g. `{}`",
            name, replacement
        );

        let range = Range {
            start: Position {
                line: line_num,
                character: col_num.saturating_sub(name_length),
            },
            end: Position {
                line: line_num,
                character: col_num,
            },
        };

        Some((
            file_uri,
            Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::WARNING),
                code: Some(NumberOrString::String(
                    DiagnosticCode::NonSnakeCase.as_str().to_string(),
                )),
                code_description: Url::from_str("https://flatbuffers.dev/schema/#style-guide")
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
