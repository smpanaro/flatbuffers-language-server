use std::{fs, path::PathBuf};

use crate::{
    diagnostics::{codes::DiagnosticCode, ErrorDiagnosticHandler},
    utils::as_pos_idx,
};
use log::error;
use regex::Regex;
use tower_lsp_server::{
    lsp_types::{
        Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range,
        Uri,
    },
    UriExt,
};

// Regex to captures duplicate definitions:
// <1file>:<2line>: <3col>: error: <4type_name> already exists: <5name> previously defined at <6original_file>:<7original_line>:<8original_col>
static DUPLICATE_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"^(.+?):(\d+): (\d+): error: (.+?) already exists: (.+?) previously defined at (.+?):(\d+):(\d+)$")
        .unwrap()
});

pub struct DuplicateDefinitionHandler;

impl ErrorDiagnosticHandler for DuplicateDefinitionHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(PathBuf, Diagnostic)> {
        if let Some(captures) = DUPLICATE_RE.captures(line) {
            let file_path = captures[1].trim();
            let Ok(file_path) = fs::canonicalize(file_path) else {
                error!("failed to canonicalize file: {file_path}");
                return None;
            };

            let name = captures[5].trim().to_string();
            let unqualified_name = name.split('.').next_back().unwrap_or(name.as_str());
            let unqualified_name_length = as_pos_idx(unqualified_name.chars().count());

            let message = format!("the name `{name}` is defined multiple times");
            let curr_line = captures[2].parse().unwrap_or(1) - 1;
            let curr_char = captures[3]
                .parse()
                .unwrap_or(0u32)
                .saturating_sub(unqualified_name_length);
            let range = Range {
                start: Position {
                    line: curr_line,
                    character: curr_char,
                },
                end: Position {
                    line: curr_line,
                    character: curr_char + unqualified_name_length,
                },
            };

            let prev_line = captures[7].parse().unwrap_or(1u32).saturating_sub(1);
            let prev_char = captures[8]
                .parse()
                .unwrap_or(0u32)
                .saturating_sub(unqualified_name_length);
            let previous_location = Location {
                uri: Uri::from_file_path(captures[6].trim()).unwrap(),
                range: Range {
                    start: Position::new(prev_line, prev_char),
                    end: Position::new(prev_line, prev_char + unqualified_name_length),
                },
            };
            Some((
                file_path,
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    message,
                    code: Some(DiagnosticCode::DuplicateDefinition.into()),
                    related_information: Some(vec![DiagnosticRelatedInformation {
                        location: previous_location,
                        message: format!("previous definition of `{name}` defined here"),
                    }]),
                    ..Default::default()
                },
            ))
        } else {
            None
        }
    }
}
