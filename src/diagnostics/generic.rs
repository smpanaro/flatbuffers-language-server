use crate::diagnostics::DiagnosticHandler;
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^.+?:(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: .+?:(\d+))?$")
        .unwrap()
});

pub struct GenericDiagnosticHandler;

impl DiagnosticHandler for GenericDiagnosticHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(Url, Diagnostic)> {
        if let Some(captures) = RE.captures(line) {
            let file_path = captures.get(0).unwrap().as_str().split(':').next().unwrap();
            let Ok(file_uri) = Url::from_file_path(file_path) else {
                error!("failed to parse file into url: {}", file_path);
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
                start: Position {
                    line: line_num,
                    character: col_num,
                },
                end: Position {
                    line: line_num,
                    character: u32::MAX,
                },
            };

            return Some((
                file_uri,
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
