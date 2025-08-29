use crate::symbol_table::{Enum, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable, Table};
use log::{debug, info};
use once_cell::sync::Lazy;
use regex::Regex;
use std::ffi::{CStr, CString};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Location, Position, Range, Url};

use crate::ffi;

/// A trait for parsing FlatBuffers schema files.
pub trait Parser {
    /// Parses a FlatBuffers schema and returns a list of diagnostics and a symbol table.
    fn parse(&self, uri: &Url, content: &str) -> (Vec<Diagnostic>, Option<SymbolTable>);
}

#[derive(Debug)]
pub struct FlatcFFIParser;

// Regex to capture: <line>:<col>: <error|warning>: <message>
static RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(\d+):\s*(\d+):\s+(error|warning):\s+(.+)$").unwrap());

impl Parser for FlatcFFIParser {
    fn parse(&self, uri: &Url, content: &str) -> (Vec<Diagnostic>, Option<SymbolTable>) {
        let c_content = match CString::new(content) {
            Ok(s) => s,
            Err(_) => return (vec![], None), // Content has null bytes
        };

        let mut diagnostics = Vec::new();
        let mut symbol_table: Option<SymbolTable> = None;

        // Unsafe block to call C++ functions
        unsafe {
            let parser_ptr = ffi::parse_schema(c_content.as_ptr());

            if parser_ptr.is_null() {
                return (diagnostics, None);
            }

            if ffi::is_parser_success(parser_ptr) {
                info!("Successfully parsed schema. Building symbol table...");
                let mut st = SymbolTable::new();

                // Handle Structs and Tables
                let num_structs = ffi::get_num_structs(parser_ptr);
                for i in 0..num_structs {
                    let def_info = ffi::get_struct_info(parser_ptr, i);
                    if def_info.name.is_null() {
                        continue;
                    }
                    let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
                    let line = (def_info.line) as u32; // TODO: fix line number

                    let symbol_kind = if def_info.is_table {
                        info!("Found table: {} at line {}", name, line + 1);
                        SymbolKind::Table(Table { fields: vec![] })
                    } else {
                        info!("Found struct: {} at line {}", name, line + 1);
                        SymbolKind::Struct(Struct { fields: vec![] })
                    };

                    let location = Location {
                        uri: uri.clone(),
                        range: Range::new(Position::new(line, 0), Position::new(line, 0)),
                    };
                    let symbol_info = SymbolInfo {
                        name: name.clone(),
                        location,
                        documentation: None,
                    };
                    st.insert(
                        name,
                        Symbol {
                            info: symbol_info,
                            kind: symbol_kind,
                        },
                    );
                }

                // Handle Enums and Unions
                let num_enums = ffi::get_num_enums(parser_ptr);
                for i in 0..num_enums {
                    let def_info = ffi::get_enum_info(parser_ptr, i);
                    if def_info.name.is_null() {
                        continue;
                    }
                    let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
                    let line = (def_info.line) as u32; // TODO: fix line number

                    let symbol_kind = if def_info.is_union {
                        info!("Found union: {} at line {}", name, line + 1);
                        // TODO: Add a Union variant to SymbolKind
                        continue;
                    } else {
                        info!("Found enum: {} at line {}", name, line + 1);
                        SymbolKind::Enum(Enum {})
                    };

                    let location = Location {
                        uri: uri.clone(),
                        range: Range::new(Position::new(line, 0), Position::new(line, 0)),
                    };
                    let symbol_info = SymbolInfo {
                        name: name.clone(),
                        location,
                        documentation: None,
                    };
                    st.insert(
                        name,
                        Symbol {
                            info: symbol_info,
                            kind: symbol_kind,
                        },
                    );
                }

                symbol_table = Some(st);
            } else {
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
            }

            // IMPORTANT: Clean up the C++ parser object
            ffi::delete_parser(parser_ptr);
        }

        (diagnostics, symbol_table)
    }
}
