use crate::symbol_table::{
    Enum, Field, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable, Table, Union,
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
    /// Parses a FlatBuffers schema and returns a list of diagnostics, a symbol table,
    /// and a list of included files.
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (Vec<Diagnostic>, Option<SymbolTable>, Vec<String>);
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

fn create_symbol(
    uri: &Url,
    name: String,
    line: u32,
    col: u32,
    kind: SymbolKind,
) -> (Symbol, Location) {
    let location = Location {
        uri: uri.clone(),
        range: Range::new(
            Position::new(line, col - (name.chars().count() as u32)),
            Position::new(line, col),
        ),
    };
    let symbol_info = SymbolInfo {
        name,
        location: location.clone(),
        documentation: None,
    };
    (
        Symbol {
            info: symbol_info,
            kind,
        },
        location,
    )
}

impl Parser for FlatcFFIParser {
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (Vec<Diagnostic>, Option<SymbolTable>, Vec<String>) {
        let scalar_types: HashSet<_> = [
            "bool", "byte", "ubyte", "short", "ushort", "int", "uint", "float", "long", "ulong",
            "double", "string",
        ]
        .iter()
        .cloned()
        .collect();

        let c_content = match CString::new(content) {
            Ok(s) => s,
            Err(_) => return (vec![], None, vec![]), // Content has null bytes
        };
        let c_filename = CString::new(uri.to_file_path().unwrap().to_str().unwrap()).unwrap();

        let mut diagnostics = Vec::new();
        let mut symbol_table: Option<SymbolTable> = None;
        let mut included_files = Vec::new();

        // Unsafe block to call C++ functions
        unsafe {
            let parser_ptr = ffi::parse_schema(c_content.as_ptr(), c_filename.as_ptr());
            if parser_ptr.is_null() {
                return (diagnostics, None, vec![]);
            }

            if ffi::is_parser_success(parser_ptr) {
                let num_included = ffi::get_num_included_files(parser_ptr);
                for i in 0..num_included {
                    let mut path_buffer = vec![0u8; 1024];
                    ffi::get_included_file_path(
                        parser_ptr,
                        i,
                        path_buffer.as_mut_ptr() as *mut i8,
                        path_buffer.len() as i32,
                    );
                    let path = CStr::from_ptr(path_buffer.as_ptr() as *const i8)
                        .to_string_lossy()
                        .into_owned();
                    if !path.is_empty() {
                        included_files.push(path);
                    }
                }
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

                    if st.contains_key(&name) {
                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new(def_info.line, def_info.col),
                                Position::new(def_info.line, def_info.col),
                            ),
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
                        let type_range = Range::new(
                            Position::new(
                                field_info.type_line,
                                field_info.type_col - (type_name.chars().count() as u32),
                            ),
                            Position::new(field_info.type_line, field_info.type_col),
                        );

                        let (field_symbol, _) = create_symbol(
                            uri,
                            field_name,
                            field_info.line,
                            field_info.col,
                            SymbolKind::Field(Field {
                                type_name,
                                type_range,
                            }),
                        );
                        fields.push(field_symbol);
                    }

                    let symbol_kind = if def_info.is_table {
                        SymbolKind::Table(Table { fields })
                    } else {
                        SymbolKind::Struct(Struct { fields })
                    };

                    let (symbol, _) =
                        create_symbol(uri, name, def_info.line, def_info.col, symbol_kind);
                    st.insert(symbol);
                }

                let num_enums = ffi::get_num_enums(parser_ptr);
                for i in 0..num_enums {
                    let def_info = ffi::get_enum_info(parser_ptr, i);
                    if def_info.name.is_null() {
                        continue;
                    }
                    let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();

                    if st.contains_key(&name) {
                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new(def_info.line, def_info.col),
                                Position::new(def_info.line, def_info.col),
                            ),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Duplicate definition: {}", name),
                            ..Default::default()
                        });
                        continue;
                    }

                    let mut variants = Vec::new();
                    let num_vals = ffi::get_num_enum_vals(parser_ptr, i);
                    for j in 0..num_vals {
                        let val_info = ffi::get_enum_val_info(parser_ptr, i, j);
                        if val_info.name.is_null() {
                            continue;
                        }
                        let val_name = CStr::from_ptr(val_info.name).to_string_lossy().into_owned();

                        // For unions, flatc adds a `NONE` member that we can skip.
                        if def_info.is_union && val_name == "NONE" {
                            continue;
                        }
                        variants.push((val_name, val_info));
                    }

                    let symbol_kind = if def_info.is_union {
                        SymbolKind::Union(Union {
                            variants: variants
                                .into_iter()
                                .map(|(name, val_info)| crate::symbol_table::UnionVariant {
                                    location: Location {
                                        uri: uri.clone(),
                                        range: Range::new(
                                            Position::new(
                                                val_info.line,
                                                val_info.col - (name.chars().count() as u32),
                                            ),
                                            Position::new(val_info.line, val_info.col),
                                        ),
                                    },
                                    name,
                                })
                                .collect(),
                        })
                    } else {
                        SymbolKind::Enum(Enum {
                            variants: variants
                                .into_iter()
                                .map(|(name, val_info)| crate::symbol_table::EnumVariant {
                                    name,
                                    value: val_info.value,
                                })
                                .collect(),
                        })
                    };

                    let (symbol, _) =
                        create_symbol(uri, name, def_info.line, def_info.col, symbol_kind);
                    st.insert(symbol);
                }

                // Second Pass: Semantic Analysis
                for symbol in st.values() {
                    let fields = match &symbol.kind {
                        SymbolKind::Table(t) => &t.fields,
                        SymbolKind::Struct(s) => &s.fields,
                        _ => continue,
                    };

                    for field in fields {
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

                symbol_table = Some(st);
            } else {
                let error_str_ptr = ffi::get_parser_error(parser_ptr);
                if !error_str_ptr.is_null() {
                    let error_c_str = CStr::from_ptr(error_str_ptr);
                    if let Ok(error_str) = error_c_str.to_str() {
                        debug!("flatc FFI error: {}", error_str);
                        for line in error_str.lines() {
                            if let Some(captures) = RE.captures(line) {
                                let line_num_str = captures.get(5).map_or_else(
                                    || captures.get(1).map_or("1", |m| m.as_str()),
                                    |m| m.as_str(),
                                );
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

        (diagnostics, symbol_table, included_files)
    }
}
