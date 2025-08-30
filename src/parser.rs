use crate::symbol_table::{
    Enum, Field, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable, Table,
};
use log::{debug, info};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;
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

// Regex to capture: <line>:<col>: <error|warning>: <message> (, originally at: :<original_line>)
static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: :(\d+))?$").unwrap()
});

fn is_known_type(type_name: &str, st: &SymbolTable, scalar_types: &HashSet<&str>) -> bool {
    // A type is known if it's a scalar or if it's in the symbol table.
    // We remove vector brackets `[]` for the check.
    let base_type_name = type_name.trim_start_matches('[').trim_end_matches(']');
    scalar_types.contains(base_type_name) || st.contains_key(base_type_name)
}

impl Parser for FlatcFFIParser {
    fn parse(&self, uri: &Url, content: &str) -> (Vec<Diagnostic>, Option<SymbolTable>) {
        let scalar_types: HashSet<_> = [
            "bool", "byte", "ubyte", "short", "ushort", "int", "uint", "float", "long", "ulong",
            "double", "string",
        ]
        .iter()
        .cloned()
        .collect();

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

                // First Pass: Collect all definitions and fields
                let num_structs = ffi::get_num_structs(parser_ptr);
                for i in 0..num_structs {
                    let def_info = ffi::get_struct_info(parser_ptr, i);
                    if def_info.name.is_null() {
                        continue;
                    }
                    let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
                    let line = (def_info.line - 1) as u32;
                    let col = (def_info.col - 1) as u32;
                    let location = Location {
                        uri: uri.clone(),
                        range: Range::new(Position::new(line, col), Position::new(line, col)),
                    };

                    if st.contains_key(&name) {
                        diagnostics.push(Diagnostic {
                            range: location.range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Duplicate definition: {}", name),
                            ..Default::default()
                        });
                        continue;
                    }

                    let mut fields = Vec::new();
                    let num_fields = ffi::get_num_fields(parser_ptr, i);
                    for j in 0..num_fields {
                        let field_info = ffi::get_field_info(parser_ptr, i, j);
                        if field_info.name.is_null() {
                            continue;
                        }

                        let field_name = CStr::from_ptr(field_info.name)
                            .to_string_lossy()
                            .into_owned();
                        let field_line = (field_info.line - 1) as u32;
                        let field_col = (field_info.col - 1) as u32;

                        let mut type_name_buffer = vec![0u8; 256];
                        ffi::get_field_type_name(
                            parser_ptr,
                            i,
                            j,
                            type_name_buffer.as_mut_ptr() as *mut i8,
                            type_name_buffer.len() as i32,
                        );
                        let type_name = CStr::from_ptr(type_name_buffer.as_ptr() as *const i8)
                            .to_string_lossy()
                            .into_owned();

                        let field_location = Location {
                            uri: uri.clone(),
                            range: Range::new(
                                Position::new(field_line, field_col),
                                Position::new(field_line, field_col),
                            ),
                        };
                        let field_symbol_info = SymbolInfo {
                            name: field_name,
                            location: field_location,
                            documentation: None,
                        };
                        let field_symbol = Symbol {
                            info: field_symbol_info,
                            kind: SymbolKind::Field(Field { type_name }),
                        };
                        fields.push(field_symbol);
                    }

                    let symbol_kind = if def_info.is_table {
                        SymbolKind::Table(Table { fields })
                    } else {
                        SymbolKind::Struct(Struct { fields })
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

                let num_enums = ffi::get_num_enums(parser_ptr);
                for i in 0..num_enums {
                    let def_info = ffi::get_enum_info(parser_ptr, i);
                    if def_info.name.is_null() {
                        continue;
                    }
                    let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
                    let line = (def_info.line - 1) as u32;
                    let col = (def_info.col - 1) as u32;
                    let location = Location {
                        uri: uri.clone(),
                        range: Range::new(Position::new(line, col), Position::new(line, col)),
                    };

                    if st.contains_key(&name) {
                        diagnostics.push(Diagnostic {
                            range: location.range,
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Duplicate definition: {}", name),
                            ..Default::default()
                        });
                        continue;
                    }

                    if !def_info.is_union {
                        let symbol_info = SymbolInfo {
                            name: name.clone(),
                            location,
                            documentation: None,
                        };
                        st.insert(
                            name,
                            Symbol {
                                info: symbol_info,
                                kind: SymbolKind::Enum(Enum {}),
                            },
                        );
                    }
                }

                // Second Pass: Semantic Analysis
                for symbol in st.values() {
                    if let SymbolKind::Table(table) = &symbol.kind {
                        for field in &table.fields {
                            if let SymbolKind::Field(field_def) = &field.kind {
                                if !is_known_type(&field_def.type_name, &st, &scalar_types) {
                                    diagnostics.push(Diagnostic {
                                        range: field.info.location.range,
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        message: format!("Undefined type: {}", field_def.type_name),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    } else if let SymbolKind::Struct(struc) = &symbol.kind {
                        for field in &struc.fields {
                            if let SymbolKind::Field(field_def) = &field.kind {
                                if !is_known_type(&field_def.type_name, &st, &scalar_types) {
                                    diagnostics.push(Diagnostic {
                                        range: field.info.location.range,
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        message: format!("Undefined type: {}", field_def.type_name),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
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
                                let line_num_str = if let Some(original_line) = captures.get(5) {
                                    original_line.as_str()
                                } else {
                                    captures.get(1).map_or("1", |m| m.as_str())
                                };
                                let line_num: u32 =
                                    line_num_str.parse().unwrap_or(1u32).saturating_sub(1);
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

            ffi::delete_parser(parser_ptr);
        }

        (diagnostics, symbol_table)
    }
}
