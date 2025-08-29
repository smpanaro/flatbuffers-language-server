use log::{debug, error};
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::Write;
use std::process::Command;
use tempfile::{Builder, NamedTempFile};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

/// A trait for parsing FlatBuffers schema files.
/// This allows for different parsing strategies to be used.
pub trait Parser {
    /// Parses a FlatBuffers schema and returns a list of diagnostics.
    fn parse(&self, uri: &Url, content: &str) -> Vec<Diagnostic>;
}

#[derive(Debug)]
pub struct FlatcCommandLineParser;

// Regex to capture: <file>:<line>:<col>: <error|warning>: <message>
// Example: `schemas/monster.fbs:8:1: error: unknown type: `MyTable`
// Example: `/Users/.._error.fbs:9: 5: error: expecting: } instead got: union`
static RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(.+?):(\d+):\s*(\d+):\s+(error|warning):\s+(.+)$").unwrap());

impl Parser for FlatcCommandLineParser {
    fn parse(&self, _uri: &Url, content: &str) -> Vec<Diagnostic> {
        let temp_dir = match Builder::new().prefix("fbs-lsp").tempdir() {
            Ok(dir) => dir,
            Err(e) => {
                error!("Failed to create temp dir: {}", e);
                return vec![];
            }
        };

        let mut temp_file = match NamedTempFile::new_in(temp_dir.path()) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to create temp file: {}", e);
                return vec![];
            }
        };

        if let Err(e) = temp_file.write_all(content.as_bytes()) {
            error!("Failed to write to temp file: {}", e);
            return vec![];
        }

        let flatc_output = match Command::new("flatc")
            .arg("-o")
            .arg(temp_dir.path())
            .arg("--cpp")
            .arg(temp_file.path())
            .output()
        {
            Ok(output) => output,
            Err(e) => {
                error!("Failed to execute flatc: {}. Is it in your PATH?", e);
                return vec![];
            }
        };

        let stderr = String::from_utf8_lossy(&flatc_output.stderr);
        debug!("flatc stderr: {}", stderr);

        let mut diagnostics = Vec::new();
        for line in stderr.lines() {
            if let Some(captures) = RE.captures(line) {
                // The file path in the error message is the temporary file.
                // The diagnostic range is what matters.
                let line_num: u32 = captures[2].parse().unwrap_or(1) - 1; // 0-indexed
                let col_num: u32 = captures[3].parse().unwrap_or(1) - 1; // 0-indexed
                let severity = if &captures[4] == "error" {
                    DiagnosticSeverity::ERROR
                } else {
                    DiagnosticSeverity::WARNING
                };
                let message = captures[5].to_string();

                let diagnostic = Diagnostic {
                    range: Range {
                        start: Position {
                            line: line_num,
                            character: col_num,
                        },
                        end: Position {
                            line: line_num,
                            character: u32::MAX,
                        }, // Go to end of line
                    },
                    severity: Some(severity),
                    message,
                    ..Default::default()
                };
                diagnostics.push(diagnostic);
            }
        }

        diagnostics
    }
}
