use std::{fs, path::PathBuf};

use crate::diagnostics::ErrorDiagnosticHandler;
use log::error;
use regex::Regex;
use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range};

static RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"^.+?:(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: .+?:(\d+))?$")
        .expect("generic diagnostic regex failed to compile")
});

pub struct GenericDiagnosticHandler;

impl ErrorDiagnosticHandler for GenericDiagnosticHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(PathBuf, Diagnostic)> {
        if let Some(captures) = RE.captures(line) {
            let file_path = captures.get(0)?.as_str().split(':').next()?;
            let Ok(file_path) = fs::canonicalize(file_path) else {
                error!("failed to canonicalize file: {file_path}");
                return None;
            };

            let line_num_str = captures.get(5).map_or_else(
                || captures.get(1).map_or("1", |m| m.as_str()),
                |m| m.as_str(),
            );
            let line_num: u32 = line_num_str.parse().unwrap_or(1u32).saturating_sub(1);
            let col_num: u32 = captures
                .get(2)
                .map_or("1", |m| m.as_str())
                .parse()
                .unwrap_or(1u32)
                .saturating_sub(1);
            let severity = if &captures[3] == "error" {
                DiagnosticSeverity::ERROR
            } else {
                DiagnosticSeverity::WARNING
            };
            let message = captures[4].trim().to_string();

            let range = Range {
                start: Position::new(line_num, col_num),
                end: Position::new(line_num, u32::MAX),
            };

            return Some((
                file_path,
                Diagnostic {
                    range,
                    severity: Some(severity),
                    message,
                    ..Default::default()
                },
            ));
        }
        None
    }
}
