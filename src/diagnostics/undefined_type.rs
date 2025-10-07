use crate::diagnostics::codes::DiagnosticCode;
use crate::diagnostics::ErrorDiagnosticHandler;
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^.+?:(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: .+?:(\d+))?$")
        .unwrap()
});

static UNDEFINED_TYPE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"type referenced but not defined \(check namespace\): ((?:\w+\.?)*)").unwrap()
});

pub struct UndefinedTypeHandler;

impl ErrorDiagnosticHandler for UndefinedTypeHandler {
    fn handle(&self, line: &str, content: &str) -> Option<(Url, Diagnostic)> {
        if let Some(captures) = RE.captures(line) {
            let message = captures[4].trim().to_string();
            if let Some(undefined_type_captures) = UNDEFINED_TYPE_RE.captures(&message) {
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

                let mut range = Range {
                    start: Position {
                        line: line_num,
                        character: col_num,
                    },
                    end: Position {
                        line: line_num,
                        character: u32::MAX,
                    },
                };

                let mut data = None;
                if let Some(type_name) = undefined_type_captures.get(1) {
                    data = Some(json!({ "type_name": type_name.as_str() }));
                    if let Some(line_content) = content.lines().nth(line_num as usize) {
                        if let Some(start) = line_content.find(type_name.as_str()) {
                            let end = start + type_name.as_str().len();
                            range.start.character = start as u32;
                            range.end.character = end as u32;
                        }
                    }
                }

                return Some((
                    file_uri,
                    Diagnostic {
                        range,
                        severity: Some(severity),
                        code: Some(tower_lsp::lsp_types::NumberOrString::String(
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
