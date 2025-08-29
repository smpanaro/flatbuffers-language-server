use log::debug;
use once_cell::sync::Lazy;
use regex::Regex;
use std::ffi::{CStr, CString};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

use crate::ffi;

/// A trait for parsing FlatBuffers schema files.
pub trait Parser {
    /// Parses a FlatBuffers schema and returns a list of diagnostics.
    fn parse(&self, uri: &Url, content: &str) -> Vec<Diagnostic>;
}

#[derive(Debug)]
pub struct FlatcFFIParser;

// Regex to capture: <line>:<col>: <error|warning>: <message>
static RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+):\s*(\d+):\s+(error|warning):\s+(.+)$").unwrap());

impl Parser for FlatcFFIParser {
    fn parse(&self, _uri: &Url, content: &str) -> Vec<Diagnostic> {
        let c_content = match CString::new(content) {
            Ok(s) => s,
            Err(_) => return vec![], // Content has null bytes
        };

        let mut diagnostics = Vec::new();

        // Unsafe block to call C++ functions
        unsafe {
            let parser_ptr = ffi::parse_schema(c_content.as_ptr());

            if !parser_ptr.is_null() {
                let error_str_ptr = ffi::get_parser_error(parser_ptr);
                if !error_str_ptr.is_null() {
                    let error_c_str = CStr::from_ptr(error_str_ptr);
                    if let Ok(error_str) = error_c_str.to_str() {
                        debug!("flatc FFI error: {}", error_str);
                        for line in error_str.lines() {
                            if let Some(captures) = RE.captures(line) {
                                let line_num: u32 = captures[1].parse().unwrap_or(1) - 1;
                                let col_num: u32 = captures[2].parse().unwrap_or(1) - 1;
                                let severity = if &captures[3] == "error" {
                                    DiagnosticSeverity::ERROR
                                } else {
                                    DiagnosticSeverity::WARNING
                                };
                                let message = captures[4].to_string();

                                let diagnostic = Diagnostic {
                                    range: Range {
                                        start: Position {
                                            line: line_num,
                                            character: col_num,
                                        },
                                        end: Position {
                                            line: line_num,
                                            character: u32::MAX,
                                        },
                                    },
                                    severity: Some(severity),
                                    message,
                                    ..Default::default()
                                };
                                diagnostics.push(diagnostic);
                            }
                        }
                    }
                }

                // IMPORTANT: Clean up the C++ parser object
                ffi::delete_parser(parser_ptr);
            }
        }

        diagnostics
    }
}
