use crate::ffi;
use crate::symbol_table::{
    Enum, EnumVariant, Field, RootTypeInfo, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable,
    Table, Union, UnionVariant,
};
use log::{debug, error};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Location,
    Position, Range, Url,
};

/// A trait for parsing FlatBuffers schema files.
pub trait Parser {
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (
        Vec<Diagnostic>,
        Option<SymbolTable>,
        Vec<String>,
        Option<RootTypeInfo>,
    );
}

#[derive(Debug)]
pub struct FlatcFFIParser;

impl Parser for FlatcFFIParser {
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (
        Vec<Diagnostic>,
        Option<SymbolTable>,
        Vec<String>,
        Option<RootTypeInfo>,
    ) {
        let c_content = match CString::new(content) {
            Ok(s) => s,
            Err(_) => return (vec![], None, vec![], None), // Content has null bytes
        };
        let c_filename = CString::new(uri.to_file_path().unwrap().to_str().unwrap()).unwrap();

        unsafe {
            let parser_ptr = ffi::parse_schema(c_content.as_ptr(), c_filename.as_ptr());
            if parser_ptr.is_null() {
                return (vec![], None, vec![], None);
            }

            let (diagnostics, symbol_table, included_files, root_type_info) =
                if ffi::is_parser_success(parser_ptr) {
                    let (st, included, root_info, diags) = parse_success_case(parser_ptr);
                    (diags, Some(st), included, root_info)
                } else {
                    (parse_error_case(parser_ptr, content), None, vec![], None)
                };

            ffi::delete_parser(parser_ptr);

            (diagnostics, symbol_table, included_files, root_type_info)
        }
    }
}

// Regex to capture: <line>:<col>: <error|warning>: <message> (, originally at: :<original_line>)
static RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^.+?:(\d+):\s*(\d+):\s+(error|warning):\s+(.+?)(?:, originally at: .+?:(\d+))?$")
        .unwrap()
});

/// Handles the successful parse case.
unsafe fn parse_success_case(
    parser_ptr: *mut ffi::FlatbuffersParser,
) -> (
    SymbolTable,
    Vec<String>,
    Option<RootTypeInfo>,
    Vec<Diagnostic>,
) {
    let mut st = SymbolTable::new();
    let mut diagnostics = Vec::new();

    let included_files = extract_included_files(parser_ptr);
    extract_structs_and_tables(parser_ptr, &mut st, &mut diagnostics);
    extract_enums_and_unions(parser_ptr, &mut st, &mut diagnostics);
    let root_type_info = extract_root_type(parser_ptr);

    let semantic_diagnostics = perform_semantic_analysis(&st);
    diagnostics.extend(semantic_diagnostics);

    (st, included_files, root_type_info, diagnostics)
}

/// Handles the error case by parsing flatc's error message.
unsafe fn parse_error_case(
    parser_ptr: *mut ffi::FlatbuffersParser,
    content: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let error_str_ptr = ffi::get_parser_error(parser_ptr);
    if !error_str_ptr.is_null() {
        let error_c_str = CStr::from_ptr(error_str_ptr);
        if let Ok(error_str) = error_c_str.to_str() {
            debug!("flatc FFI error: {}", error_str);
            for line in error_str.lines() {
                if let Some(already_define_diag) = parse_already_defined(line, content) {
                    diagnostics.push(already_define_diag);
                } else if let Some(captures) = RE.captures(line) {
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

                    let undefined_type_re =
                        Regex::new(r"type referenced but not defined \(check namespace\): (\w+)")
                            .unwrap();
                    if let Some(captures) = undefined_type_re.captures(&message) {
                        if let Some(type_name) = captures.get(1) {
                            if let Some(line_content) = content.lines().nth(line_num as usize) {
                                if let Some(start) = line_content.find(type_name.as_str()) {
                                    let end = start + type_name.as_str().len();
                                    range.start.character = start as u32;
                                    range.end.character = end as u32;
                                }
                            }
                        }
                    }

                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(severity),
                        message,
                        ..Default::default()
                    });
                }
            }
        }
    }
    diagnostics
}

// Regex to captures duplicate definitions:
// <1file>:<2line>: <3col>: error: <4type_name> already exists: <5name> previously defined at <6original_file>:<7original_line>:<8original_col>
static DUPLICATE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(.+?):(\d+): (\d+): error: ([a-z\s]+) already exists: (.+?) previously defined at (.+?):(\d+):(\d+)$")
        .unwrap()
});

fn parse_already_defined(line: &str, content: &str) -> Option<Diagnostic> {
    if let Some(captures) = DUPLICATE_RE.captures(line) {
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

        let prev_line = captures[7].parse().unwrap_or(0) - 1;
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
        Some(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            message,
            related_information: Some(vec![DiagnosticRelatedInformation {
                location: previous_location,
                message: format!("previous definition of `{}` defined here", name),
            }]),
            ..Default::default()
        })
    } else {
        None
    }
}

/// Extracts all included file paths from the parser.
unsafe fn extract_included_files(parser_ptr: *mut ffi::FlatbuffersParser) -> Vec<String> {
    let mut included_files = Vec::new();
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
    included_files
}

/// Extracts all struct and table definitions from the parser.
unsafe fn extract_structs_and_tables(
    parser_ptr: *mut ffi::FlatbuffersParser,
    st: &mut SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let num_structs = ffi::get_num_structs(parser_ptr);
    for i in 0..num_structs {
        let def_info = ffi::get_struct_info(parser_ptr, i);
        if def_info.name.is_null() {
            continue;
        }
        let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
        let file = CStr::from_ptr(def_info.file).to_string_lossy().into_owned();
        let Ok(file_uri) = Url::from_file_path(&file) else {
            error!("failed to parse file into url: {}", file);
            continue;
        };

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

            let doc = ffi_get_string(|buf, len| {
                ffi::get_field_documentation(parser_ptr, i, j, buf, len);
            });
            let documentation = if doc.is_empty() { None } else { Some(doc) };

            let (field_symbol, _) = create_symbol(
                &file_uri,
                field_name,
                field_info.line,
                field_info.col,
                SymbolKind::Field(Field {
                    type_name,
                    type_range,
                    deprecated: field_info.deprecated,
                }),
                documentation,
            );
            fields.push(field_symbol);
        }

        let symbol_kind = if def_info.is_table {
            SymbolKind::Table(Table { fields })
        } else {
            SymbolKind::Struct(Struct {
                fields,
                size: def_info.bytesize as usize,
                alignment: def_info.minalign as usize,
            })
        };

        let doc = ffi_get_string(|buf, len| {
            ffi::get_struct_documentation(parser_ptr, i, buf, len);
        });
        let documentation = if doc.is_empty() { None } else { Some(doc) };

        let (symbol, _) = create_symbol(
            &file_uri,
            name,
            def_info.line,
            def_info.col,
            symbol_kind,
            documentation,
        );
        st.insert(symbol);
    }
}

/// Extracts all enum and union definitions from the parser.
unsafe fn extract_enums_and_unions(
    parser_ptr: *mut ffi::FlatbuffersParser,
    st: &mut SymbolTable,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let num_enums = ffi::get_num_enums(parser_ptr);
    for i in 0..num_enums {
        let def_info = ffi::get_enum_info(parser_ptr, i);
        if def_info.name.is_null() {
            continue;
        }
        let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();
        let file = CStr::from_ptr(def_info.file).to_string_lossy().into_owned();
        let Ok(file_uri) = Url::from_file_path(&file) else {
            error!("failed to parse file into url: {}", file);
            continue;
        };

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

            if def_info.is_union && val_name == "NONE" {
                continue;
            }
            variants.push((val_name, val_info));
        }

        let symbol_kind = if def_info.is_union {
            SymbolKind::Union(Union {
                variants: variants
                    .into_iter()
                    .map(|(name, val_info)| UnionVariant {
                        location: Location {
                            uri: file_uri.clone(),
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
                    .enumerate()
                    .map(|(j, (name, val_info))| {
                        let doc = ffi_get_string(|buf, len| {
                            ffi::get_enum_val_documentation(parser_ptr, i, j as i32, buf, len);
                        });
                        let documentation = if doc.is_empty() { None } else { Some(doc) };
                        EnumVariant {
                            name,
                            value: val_info.value,
                            documentation,
                        }
                    })
                    .collect(),
            })
        };

        let doc = ffi_get_string(|buf, len| ffi::get_enum_documentation(parser_ptr, i, buf, len));
        let documentation = if doc.is_empty() { None } else { Some(doc) };

        let (symbol, _) = create_symbol(
            &file_uri,
            name,
            def_info.line,
            def_info.col,
            symbol_kind,
            documentation,
        );
        st.insert(symbol);
    }
}

/// Extracts the root type definition from the parser.
unsafe fn extract_root_type(parser_ptr: *mut ffi::FlatbuffersParser) -> Option<RootTypeInfo> {
    if ffi::has_root_type(parser_ptr) {
        let root_def = ffi::get_root_type_info(parser_ptr);
        let name = CStr::from_ptr(root_def.name).to_string_lossy().into_owned();
        let file = CStr::from_ptr(root_def.file).to_string_lossy().into_owned();
        if let Ok(file_uri) = Url::from_file_path(&file) {
            let location = Location {
                uri: file_uri,
                range: Range::new(
                    Position::new(root_def.line, root_def.col - (name.chars().count() as u32)),
                    Position::new(root_def.line, root_def.col),
                ),
            };
            return Some(RootTypeInfo {
                location,
                type_name: name,
            });
        }
    }
    None
}

/// Performs second-pass semantic analysis on the symbol table.
fn perform_semantic_analysis(st: &SymbolTable) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let scalar_types: HashSet<_> = [
        "bool", "byte", "ubyte", "int8", "uint8", "short", "ushort", "int16", "uint16", "int",
        "uint", "int32", "uint32", "float", "float32", "long", "ulong", "int64", "uint64",
        "double", "float64", "string",
    ]
    .iter()
    .cloned()
    .collect();

    for symbol in st.values() {
        let fields = match &symbol.kind {
            SymbolKind::Table(t) => &t.fields,
            SymbolKind::Struct(s) => &s.fields,
            _ => continue,
        };

        for field in fields {
            if let SymbolKind::Field(field_def) = &field.kind {
                if !is_known_type(&field_def.type_name, st, &scalar_types) {
                    diagnostics.push(Diagnostic {
                        range: field.info.location.range,
                        severity: DiagnosticSeverity::ERROR.into(),
                        message: format!("Undefined type: {}", field_def.type_name),
                        ..Default::default()
                    });
                }
                if field_def.deprecated {
                    diagnostics.push(Diagnostic {
                        range: Range {
                            start: field.info.location.range.start,
                            end: Position {
                                line: field.info.location.range.end.line,
                                character: u32::MAX,
                            },
                        },
                        severity: DiagnosticSeverity::HINT.into(),
                        tags: vec![DiagnosticTag::UNNECESSARY].into(),
                        message: "Deprecated. Excluded from generated code.".to_string(),
                        ..Default::default()
                    })
                }
            }
        }
    }
    diagnostics
}

/// Helper to check if a type is a builtin scalar or defined in the symbol table.
fn is_known_type(type_name: &str, st: &SymbolTable, scalar_types: &HashSet<&str>) -> bool {
    let base_type_name = if let Some(stripped) = type_name.strip_prefix('[') {
        if let Some(end_bracket) = stripped.rfind(']') {
            let inner = &stripped[..end_bracket];
            if let Some(colon_pos) = inner.find(':') {
                &inner[..colon_pos]
            } else {
                inner
            }
        } else {
            type_name
        }
    } else {
        type_name
    };
    scalar_types.contains(base_type_name) || st.contains_key(base_type_name)
}

/// Helper to create a symbol and its location.
fn create_symbol(
    uri: &Url,
    name: String,
    line: u32,
    col: u32,
    kind: SymbolKind,
    documentation: Option<String>,
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
        documentation,
    };
    (
        Symbol {
            info: symbol_info,
            kind,
        },
        location,
    )
}

/// Helper to safely get a string from an FFI function that uses a character buffer.
unsafe fn ffi_get_string(getter: impl Fn(*mut i8, i32)) -> String {
    let mut buffer = vec![0u8; 2048];
    getter(buffer.as_mut_ptr() as *mut i8, buffer.len() as i32);
    CStr::from_ptr(buffer.as_ptr() as *const i8)
        .to_string_lossy()
        .into_owned()
}
