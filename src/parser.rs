use crate::diagnostics::{self, DiagnosticHandler};
use crate::ffi;
use crate::symbol_table::{
    Enum, EnumVariant, Field, RootTypeInfo, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable,
    Table, Union, UnionVariant,
};
use crate::utils::parsed_type::parse_type;
use log::{debug, error};
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use tower_lsp::lsp_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, Location, Position, Range, Url,
};

/// A trait for parsing FlatBuffers schema files.
pub trait Parser {
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (
        HashMap<Url, Vec<Diagnostic>>,
        Option<SymbolTable>,
        Vec<String>,
        Option<RootTypeInfo>,
    );
}

#[derive(Debug, Clone, Copy)]
pub struct FlatcFFIParser;

impl Parser for FlatcFFIParser {
    fn parse(
        &self,
        uri: &Url,
        content: &str,
    ) -> (
        HashMap<Url, Vec<Diagnostic>>,
        Option<SymbolTable>,
        Vec<String>,
        Option<RootTypeInfo>,
    ) {
        let c_content = match CString::new(content) {
            Ok(s) => s,
            Err(_) => return (HashMap::new(), None, vec![], None), // Content has null bytes
        };
        let c_filename = CString::new(uri.to_file_path().unwrap().to_str().unwrap()).unwrap();

        unsafe {
            let parser_ptr = ffi::parse_schema(c_content.as_ptr(), c_filename.as_ptr());
            if parser_ptr.is_null() {
                return (HashMap::new(), None, vec![], None);
            }

            let (diagnostics, symbol_table, included_files, root_type_info) =
                if ffi::is_parser_success(parser_ptr) {
                    let (st, included, root_info, diags) =
                        parse_success_case(parser_ptr, uri, content);
                    (diags, Some(st), included, root_info)
                } else {
                    (
                        parse_error_case(parser_ptr, &uri.to_string(), content),
                        None,
                        extract_all_included_files(parser_ptr),
                        None,
                    )
                };

            ffi::delete_parser(parser_ptr);

            (diagnostics, symbol_table, included_files, root_type_info)
        }
    }
}

/// Handles the successful parse case.
unsafe fn parse_success_case(
    parser_ptr: *mut ffi::FlatbuffersParser,
    uri: &Url,
    content: &str,
) -> (
    SymbolTable,
    Vec<String>,
    Option<RootTypeInfo>,
    HashMap<Url, Vec<Diagnostic>>,
) {
    let mut st = SymbolTable::new(uri.clone());
    let mut diagnostics = HashMap::new();

    let included_files = extract_all_included_files(parser_ptr);

    extract_structs_and_tables(parser_ptr, &mut st, &mut diagnostics);
    extract_enums_and_unions(parser_ptr, &mut st, &mut diagnostics);
    let root_type_info = extract_root_type(parser_ptr);

    perform_semantic_analysis(&st, &mut diagnostics, &included_files, content, parser_ptr);

    (st, included_files, root_type_info, diagnostics)
}

/// Handles the error case by parsing flatc's error message.
unsafe fn parse_error_case(
    parser_ptr: *mut ffi::FlatbuffersParser,
    file_name: &str,
    content: &str,
) -> HashMap<Url, Vec<Diagnostic>> {
    let mut diagnostics_map: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    let error_str_ptr = ffi::get_parser_error(parser_ptr);

    if !error_str_ptr.is_null() {
        let error_c_str = CStr::from_ptr(error_str_ptr);
        if let Ok(error_str) = error_c_str.to_str() {
            debug!("flatc FFI error parsing {}: {}", file_name, error_str);

            let handlers: Vec<Box<dyn DiagnosticHandler>> = vec![
                Box::new(diagnostics::duplicate_definition::DuplicateDefinitionHandler),
                Box::new(diagnostics::expecting_token::ExpectingTokenHandler),
                Box::new(diagnostics::undefined_type::UndefinedTypeHandler),
                Box::new(diagnostics::generic::GenericDiagnosticHandler),
            ];

            for line in error_str.lines() {
                for handler in &handlers {
                    if let Some((file_uri, diagnostic)) = handler.handle(line, content) {
                        diagnostics_map
                            .entry(file_uri)
                            .or_default()
                            .push(diagnostic);
                        break;
                    }
                }
            }
        }
    }
    diagnostics_map
}

/// Extracts all included file paths from the parser.
unsafe fn extract_all_included_files(parser_ptr: *mut ffi::FlatbuffersParser) -> Vec<String> {
    let mut included_files = Vec::new();
    let num_included = ffi::get_num_all_included_files(parser_ptr);
    for i in 0..num_included {
        let mut path_buffer = vec![0u8; 1024];
        ffi::get_all_included_file_path(
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
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
) {
    let num_structs = ffi::get_num_structs(parser_ptr);
    for i in 0..num_structs {
        let def_info = ffi::get_struct_info(parser_ptr, i);
        if def_info.name.is_null() {
            continue;
        }
        let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();

        let mut namespace_buffer = vec![0u8; 256];
        ffi::get_struct_namespace(
            parser_ptr,
            i,
            namespace_buffer.as_mut_ptr() as *mut i8,
            namespace_buffer.len() as i32,
        );
        let namespace_name = CStr::from_ptr(namespace_buffer.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned();

        let namespace: Vec<String> = if namespace_name.is_empty() {
            vec![]
        } else {
            namespace_name.split('.').map(|s| s.to_string()).collect()
        };

        let qualified_name = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", namespace.join("."), name)
        };

        let file = CStr::from_ptr(def_info.file).to_string_lossy().into_owned();
        let Ok(file_uri) = Url::from_file_path(&file) else {
            error!("failed to parse file into url: {}", file);
            continue;
        };

        if st.contains_key(&qualified_name) {
            diagnostics
                .entry(file_uri.clone())
                .or_default()
                .push(Diagnostic {
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

            let type_name = ffi_get_string(|buf, len| {
                ffi::get_field_base_type_name(parser_ptr, i, j, buf, len);
            });
            let type_display_name = ffi_get_string(|buf, len| {
                ffi::get_field_type_name(parser_ptr, i, j, buf, len);
            });

            let type_source = CStr::from_ptr(field_info.type_source)
                .to_string_lossy()
                .into_owned();

            let type_range = field_info.type_range.into();
            let parsed_type = parse_type(&type_source, type_range);

            let doc = ffi_get_string(|buf, len| {
                ffi::get_field_documentation(parser_ptr, i, j, buf, len);
            });
            let documentation = if doc.is_empty() { None } else { Some(doc) };

            let (field_symbol, _) = create_symbol(
                &file_uri,
                field_name.clone(),
                vec![], // Fields do not have namespaces themselves
                field_info.line,
                field_info.col,
                SymbolKind::Field(Field {
                    type_name,
                    type_display_name,
                    type_range,
                    parsed_type,
                    deprecated: field_info.deprecated,
                    has_id: field_info.has_id,
                    id: field_info.id,
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
            namespace,
            def_info.line,
            def_info.col,
            symbol_kind,
            documentation,
        );
        st.insert(qualified_name, symbol);
    }
}

/// Extracts all enum and union definitions from the parser.
unsafe fn extract_enums_and_unions(
    parser_ptr: *mut ffi::FlatbuffersParser,
    st: &mut SymbolTable,
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
) {
    let num_enums = ffi::get_num_enums(parser_ptr);
    for i in 0..num_enums {
        let def_info = ffi::get_enum_info(parser_ptr, i);
        if def_info.name.is_null() {
            continue;
        }
        let name = CStr::from_ptr(def_info.name).to_string_lossy().into_owned();

        let mut namespace_buffer = vec![0u8; 256];
        ffi::get_enum_namespace(
            parser_ptr,
            i,
            namespace_buffer.as_mut_ptr() as *mut i8,
            namespace_buffer.len() as i32,
        );
        let namespace_name = CStr::from_ptr(namespace_buffer.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned();

        let namespace: Vec<String> = if namespace_name.is_empty() {
            vec![]
        } else {
            namespace_name.split('.').map(|s| s.to_string()).collect()
        };

        let qualified_name = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", namespace.join("."), name)
        };

        let file = CStr::from_ptr(def_info.file).to_string_lossy().into_owned();
        let Ok(file_uri) = Url::from_file_path(&file) else {
            error!("failed to parse file into url: {}", file);
            continue;
        };

        if st.contains_key(&qualified_name) {
            diagnostics
                .entry(file_uri.clone())
                .or_default()
                .push(Diagnostic {
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

            if def_info.is_union && (val_name == "NONE" || val_name == "") {
                continue;
            }
            variants.push((val_name, val_info));
        }

        let symbol_kind = if def_info.is_union {
            SymbolKind::Union(Union {
                variants: variants
                    .into_iter()
                    .map(|(name, val_info)| {
                        let type_source = CStr::from_ptr(val_info.type_source)
                            .to_string_lossy()
                            .into_owned();
                        let type_range = val_info.type_range.into();
                        let parsed_type = parse_type(&type_source, type_range);
                        let location = Location {
                            uri: file_uri.clone(),
                            range: type_range,
                        };
                        UnionVariant {
                            name,
                            location,
                            parsed_type,
                        }
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
            namespace,
            def_info.line,
            def_info.col,
            symbol_kind,
            documentation,
        );
        st.insert(qualified_name, symbol);
    }
}

/// Extracts the root type definition from the parser.
unsafe fn extract_root_type(parser_ptr: *mut ffi::FlatbuffersParser) -> Option<RootTypeInfo> {
    if ffi::has_root_type(parser_ptr) {
        let root_def = ffi::get_root_type_info(parser_ptr);
        let qualified_name = CStr::from_ptr(root_def.name).to_string_lossy().into_owned();
        let file = CStr::from_ptr(root_def.file).to_string_lossy().into_owned();

        if let Ok(file_uri) = Url::from_file_path(&file) {
            let type_source = CStr::from_ptr(root_def.type_source)
                .to_string_lossy()
                .into_owned();
            let type_range = root_def.type_range.into();
            let parsed_type = parse_type(&type_source, type_range);

            let location = Location {
                uri: file_uri,
                range: type_range,
            };
            return Some(RootTypeInfo {
                location,
                type_name: qualified_name,
                parsed_type,
            });
        }
    }
    None
}

/// Performs second-pass semantic analysis on the symbol table.
fn perform_semantic_analysis(
    st: &SymbolTable,
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
    _included_files: &Vec<String>,
    file_contents: &str,
    parser_ptr: *mut ffi::FlatbuffersParser,
) {
    let scalar_types: HashSet<_> = [
        "bool", "byte", "ubyte", "int8", "uint8", "short", "ushort", "int16", "uint16", "int",
        "uint", "int32", "uint32", "float", "float32", "long", "ulong", "int64", "uint64",
        "double", "float64", "string",
    ]
    .iter()
    .cloned()
    .collect();

    let mut used_types = HashSet::new();
    for symbol in st.values() {
        if symbol.info.location.uri != st.uri {
            continue;
        }
        match &symbol.kind {
            SymbolKind::Table(t) => {
                for field in &t.fields {
                    if let SymbolKind::Field(f) = &field.kind {
                        used_types.insert(f.type_name.clone());
                    }
                }
            }
            SymbolKind::Struct(s) => {
                for field in &s.fields {
                    if let SymbolKind::Field(f) = &field.kind {
                        used_types.insert(f.type_name.clone());
                    }
                }
            }
            SymbolKind::Union(u) => {
                for variant in &u.variants {
                    used_types.insert(variant.name.clone());
                }
            }
            _ => continue,
        }
    }

    let include_graph = unsafe { build_include_graph(parser_ptr) };

    let mut directly_required_files = HashSet::new();
    for used_type in &used_types {
        if let Some(symbol) = st.get(used_type) {
            directly_required_files.insert(symbol.info.location.uri.to_file_path().unwrap());
        }
    }

    let mut required_files = HashSet::new();
    let mut queue: Vec<_> = directly_required_files.into_iter().collect();
    let mut visited = HashSet::new();

    while let Some(file) = queue.pop() {
        if visited.contains(&file) {
            continue;
        }
        visited.insert(file.clone());
        required_files.insert(file.clone());

        if let Some(includes) = include_graph.get(file.to_str().unwrap()) {
            for include in includes {
                let mut path = std::path::PathBuf::new();
                path.push(include);
                queue.push(path);
            }
        }
    }

    for (line_num, line) in file_contents.lines().enumerate() {
        if line.starts_with("include") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line.rfind('"') {
                    let include_path = &line[start + 1..end];
                    let mut absolute_path = st.uri.to_file_path().unwrap();
                    absolute_path.pop();
                    absolute_path.push(include_path);

                    if !required_files.contains(&absolute_path) {
                        let range = Range {
                            start: Position {
                                line: line_num as u32,
                                character: 0,
                            },
                            end: Position {
                                line: line_num as u32,
                                character: line.len() as u32,
                            },
                        };
                        diagnostics
                            .entry(st.uri.clone())
                            .or_default()
                            .push(Diagnostic {
                                range,
                                severity: Some(DiagnosticSeverity::HINT),
                                message: format!("unused include: {}", include_path),
                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                ..Default::default()
                            });
                    }
                }
            }
        }
    }

    for symbol in st.values() {
        let fields = match &symbol.kind {
            SymbolKind::Table(t) => &t.fields,
            SymbolKind::Struct(s) => &s.fields,
            _ => continue,
        };

        for field in fields {
            if let SymbolKind::Field(field_def) = &field.kind {
                if !is_known_type(&field_def.type_name, st, &scalar_types) {
                    diagnostics
                        .entry(field.info.location.uri.clone())
                        .or_default()
                        .push(Diagnostic {
                            range: field.info.location.range,
                            severity: DiagnosticSeverity::ERROR.into(),
                            message: format!("Undefined type: {}", field_def.type_name),
                            ..Default::default()
                        });
                }
                if field_def.deprecated {
                    diagnostics
                        .entry(field.info.location.uri.clone())
                        .or_default()
                        .push(Diagnostic {
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
                        });
                }
            }
        }
    }
}

unsafe fn build_include_graph(
    parser_ptr: *mut ffi::FlatbuffersParser,
) -> HashMap<String, Vec<String>> {
    let mut include_graph = HashMap::new();
    let num_files = ffi::get_num_files_with_includes(parser_ptr);
    for i in 0..num_files {
        let mut path_buffer = vec![0u8; 1024];
        ffi::get_file_with_includes_path(
            parser_ptr,
            i,
            path_buffer.as_mut_ptr() as *mut i8,
            path_buffer.len() as i32,
        );
        let file_path = CStr::from_ptr(path_buffer.as_ptr() as *const i8)
            .to_string_lossy()
            .into_owned();

        let c_file_path = CString::new(file_path.clone()).unwrap();
        let num_includes = ffi::get_num_includes_for_file(parser_ptr, c_file_path.as_ptr());
        let mut includes = Vec::new();
        for j in 0..num_includes {
            let mut include_path_buffer = vec![0u8; 1024];
            ffi::get_included_file_path(
                parser_ptr,
                c_file_path.as_ptr(),
                j,
                include_path_buffer.as_mut_ptr() as *mut i8,
                include_path_buffer.len() as i32,
            );
            let include_path = CStr::from_ptr(include_path_buffer.as_ptr() as *const i8)
                .to_string_lossy()
                .into_owned();
            includes.push(include_path);
        }
        include_graph.insert(file_path, includes);
    }
    include_graph
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
    namespace: Vec<String>,
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
        namespace,
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
