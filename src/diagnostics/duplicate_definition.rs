use crate::diagnostics::ErrorDiagnosticHandler;
use log::error;
use once_cell::sync::Lazy;
use regex::Regex;
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, Position, Range, Url,
};

// Regex to captures duplicate definitions:
// <1file>:<2line>: <3col>: error: <4type_name> already exists: <5name> previously defined at <6original_file>:<7original_line>:<8original_col>
static DUPLICATE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?):(\d+): (\d+): error: (.+?) already exists: (.+?) previously defined at (.+?):(\d+):(\d+)$")
        .unwrap()
});

pub struct DuplicateDefinitionHandler;

impl ErrorDiagnosticHandler for DuplicateDefinitionHandler {
    fn handle(&self, line: &str, _content: &str) -> Option<(Url, Diagnostic)> {
        if let Some(captures) = DUPLICATE_RE.captures(line) {
            let file_path = captures[1].trim();
            let Ok(file_uri) = Url::from_file_path(file_path) else {
                error!("failed to parse file into url: {}", file_path);
                return None;
            };

            let name = captures[5].trim().to_string();
            let unqualified_name = name.split('.').last().unwrap_or(name.as_str());
            let unqualified_name_length = unqualified_name.chars().count() as u32;

            let message = format!("the name `{}` is defined multiple times", name);
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
                uri: Url::from_file_path(captures[6].trim()).unwrap(),
                range: Range {
                    start: Position {
                        line: prev_line,
                        character: prev_char,
                    },
                    end: Position {
                        line: prev_line,
                        character: prev_char + unqualified_name_length,
                    },
                },
            };
            Some((
                file_uri,
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    message,
                    related_information: Some(vec![DiagnosticRelatedInformation {
                        location: previous_location,
                        message: format!("previous definition of `{}` defined here", name),
                    }]),
                    ..Default::default()
                },
            ))
        } else {
            None
        }
    }
}
