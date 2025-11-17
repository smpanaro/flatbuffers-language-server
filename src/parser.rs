use crate::diagnostics;
use crate::ffi;
use crate::symbol_table::RpcMethod;
use crate::symbol_table::RpcMethodType;
use crate::symbol_table::RpcService;
use crate::symbol_table::{
    Enum, EnumVariant, Field, RootTypeInfo, Struct, Symbol, SymbolInfo, SymbolKind, SymbolTable,
    Table, Union, UnionVariant,
};
use crate::utils::as_pos_idx;
use crate::utils::parsed_type::parse_type;
use log::{debug, error};
use std::collections::HashMap;
use std::ffi::c_char;
use std::ffi::{CStr, CString};
use std::fs;
use std::path::{Path, PathBuf};
use std::string::ToString;
use tower_lsp_server::lsp_types::{Diagnostic, Position, Range};

#[derive(Default)]
pub struct ParseResult {
    pub diagnostics: HashMap<PathBuf, Vec<Diagnostic>>,
    pub symbol_table: Option<SymbolTable>,
    pub includes: Vec<PathBuf>,
    pub root_type_info: Option<RootTypeInfo>,
}

/// A trait for parsing `FlatBuffers` schema files.
pub trait Parser {
    fn parse(&self, path: &Path, content: &str, search_paths: &[PathBuf]) -> ParseResult;
}

#[derive(Debug, Clone, Copy)]
pub struct FlatcFFIParser;

impl Parser for FlatcFFIParser {
    fn parse(&self, path: &Path, content: &str, search_paths: &[PathBuf]) -> ParseResult {
        let Ok(c_content) = CString::new(content) else {
            return ParseResult::default();
        };
        let c_filename = CString::new(path.to_str().unwrap_or_default()).unwrap();

        let c_search_paths: Vec<CString> = search_paths
            .iter()
            .filter_map(|path| CString::new(path.to_str().unwrap_or_default()).ok())
            .collect();

        let mut c_path_ptrs: Vec<*const c_char> =
            c_search_paths.iter().map(|s| s.as_ptr()).collect();
        c_path_ptrs.push(std::ptr::null());

        unsafe {
            let parser_ptr = ffi::parse_schema(
                c_content.as_ptr(),
                c_filename.as_ptr(),
                c_path_ptrs.as_mut_ptr(),
            );
            if parser_ptr.is_null() {
                return ParseResult::default();
            }

            let mut diagnostics = parse_error_messages(parser_ptr, path, content);

            let mut st = SymbolTable::new(path.to_path_buf());
            extract_structs_and_tables(parser_ptr, &mut st);
            extract_enums_and_unions(parser_ptr, &mut st);
            extract_rpc_services(parser_ptr, &mut st);

            let included_files = extract_all_included_files(parser_ptr); // recursive. includes transient includes.
            let root_type_info = extract_root_type(parser_ptr);

            let include_graph = build_include_graph(parser_ptr); // direct includes only.
            diagnostics::semantic::analyze_unused_includes(
                &st,
                &mut diagnostics,
                content,
                &include_graph,
                search_paths,
                &root_type_info,
            );
            diagnostics::semantic::analyze_deprecated_fields(&st, &mut diagnostics);

            let result = ParseResult {
                diagnostics,
                symbol_table: Some(st),
                includes: included_files,
                root_type_info,
            };

            ffi::delete_parser(parser_ptr);

            result
        }
    }
}

/// Parse flatc's error messages (in the error case) or warnings (in the success case).
unsafe fn parse_error_messages(
    parser_ptr: *mut ffi::FlatbuffersParser,
    path: &Path,
    content: &str,
) -> HashMap<PathBuf, Vec<Diagnostic>> {
    let error_str_ptr = ffi::get_parser_error(parser_ptr);
    if let Some(error_str) = c_str_to_optional_string(error_str_ptr).take_if(|s| !s.is_empty()) {
        debug!("flatc error parsing {}: {}", path.display(), error_str);
        diagnostics::generate_diagnostics_from_error_string(&error_str, path, content)
    } else {
        HashMap::new()
    }
}

/// Extracts all included file paths from the parser.
unsafe fn extract_all_included_files(parser_ptr: *mut ffi::FlatbuffersParser) -> Vec<PathBuf> {
    let mut included_files = Vec::new();
    let num_included = ffi::get_num_all_included_files(parser_ptr);
    for i in 0..num_included {
        if let Some(path) = c_str_to_optional_string(ffi::get_all_included_file_path(parser_ptr, i))
            .and_then(|p| fs::canonicalize(&p).ok())
        {
            included_files.push(path);
        }
    }
    included_files
}

/// Extracts all struct and table definitions from the parser.
unsafe fn extract_structs_and_tables(
    parser_ptr: *mut ffi::FlatbuffersParser,
    st: &mut SymbolTable,
) {
    let num_structs = ffi::get_num_structs(parser_ptr);
    for i in 0..num_structs {
        let def_info = ffi::get_struct_info(parser_ptr, i);
        let Some(name) = c_str_to_optional_string(def_info.name) else {
            continue;
        };

        let namespace: Vec<String> = c_str_to_optional_string(def_info.namespace_)
            .map(|s| s.split('.').map(ToString::to_string).collect())
            .unwrap_or_default();

        let qualified_name = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", namespace.join("."), name)
        };

        let file = c_str_to_string(def_info.file);
        let Ok(file_path) = fs::canonicalize(&file) else {
            error!("failed to canonicalize file: {file}");
            continue;
        };

        if st.contains_key(&qualified_name) {
            // This should not happen. The flatbuffers parser returns rich errors for duplicate definitions.
            error!("found duplicate symbol while extracting structs: {qualified_name}");
            continue;
        }

        let mut fields = Vec::new();
        let num_fields = ffi::get_num_fields(parser_ptr, i);
        for j in 0..num_fields {
            let field_info = ffi::get_field_info(parser_ptr, i, j);
            let Some(field_name) = c_str_to_optional_string(field_info.name) else {
                continue;
            };

            let type_name = c_str_to_string(field_info.base_type_name);
            let type_display_name = c_str_to_string(field_info.type_name);

            let type_source = c_str_to_string(field_info.type_source);

            let type_range = field_info.type_range.into();
            let Some(parsed_type) = parse_type(&type_source, type_range) else {
                error!("Failed to parse field type at {}:{}:{}. Please open a GitHub Issue: https://github.com/smpanaro/flatbuffers-language-server/issues",
                    file_path.display(), type_range.end.line, type_range.end.character);
                continue;
            };

            let documentation = c_str_to_optional_string(field_info.documentation);

            let field_symbol = create_symbol(
                &file_path,
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
                    id: Some(field_info.id).take_if(|_| field_info.has_id),
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
                size: def_info.bytesize,
                alignment: def_info.minalign,
            })
        };

        let documentation = c_str_to_optional_string(def_info.documentation);

        let symbol = create_symbol(
            &file_path,
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
unsafe fn extract_enums_and_unions(parser_ptr: *mut ffi::FlatbuffersParser, st: &mut SymbolTable) {
    let num_enums = ffi::get_num_enums(parser_ptr);
    for i in 0..num_enums {
        let def_info = ffi::get_enum_info(parser_ptr, i);
        let Some(name) = c_str_to_optional_string(def_info.name) else {
            continue;
        };

        let namespace: Vec<String> = c_str_to_optional_string(def_info.namespace_)
            .map(|s| s.split('.').map(ToString::to_string).collect())
            .unwrap_or_default();

        let qualified_name = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", namespace.join("."), name)
        };

        let file = c_str_to_string(def_info.file);
        let Ok(file_path) = fs::canonicalize(&file) else {
            error!("failed to canonicalize file: {file}");
            continue;
        };

        if st.contains_key(&qualified_name) {
            // This should not happen. The flatbuffers parser returns rich errors for duplicate definitions.
            error!("found duplicate symbol while extracting enums: {qualified_name}");
            continue;
        }

        let mut variants = Vec::new();
        let num_vals = ffi::get_num_enum_vals(parser_ptr, i);
        for j in 0..num_vals {
            let val_info = ffi::get_enum_val_info(parser_ptr, i, j);
            let Some(val_name) = c_str_to_optional_string(val_info.name) else {
                continue;
            };

            if def_info.is_union && (val_name == "NONE" || val_name.is_empty()) {
                continue;
            }
            variants.push((val_name, val_info));
        }

        let underlying_type = c_str_to_string(def_info.underlying_type);

        let symbol_kind = if def_info.is_union {
            SymbolKind::Union(Union {
                variants: variants
                    .into_iter()
                    .filter_map(|(name, val_info)| {
                        let type_source = c_str_to_string(val_info.type_source);
                        let type_range = val_info.type_range.into();
                        let Some(parsed_type) = parse_type(&type_source, type_range) else {
                            error!("Failed to parse union variant type at {}:{}:{}. Please open a GitHub Issue: https://github.com/smpanaro/flatbuffers-language-server/issues",
                                file_path.display(), type_range.end.line, type_range.end.character);
                            return None
                        };
                        let location = crate::symbol_table::Location {
                            path: file_path.clone(),
                            range: type_range,
                        };
                        Some(UnionVariant {
                            name,
                            location,
                            parsed_type,
                        })
                    })
                    .collect(),
            })
        } else {
            SymbolKind::Enum(Enum {
                variants: variants
                    .into_iter()
                    .map(|(name, val_info)| {
                        let documentation = c_str_to_optional_string(val_info.documentation);
                        EnumVariant {
                            name,
                            value: val_info.value,
                            documentation,
                        }
                    })
                    .collect(),
                underlying_type,
            })
        };

        let documentation = c_str_to_optional_string(def_info.documentation);

        let symbol = create_symbol(
            &file_path,
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

unsafe fn extract_rpc_services(parser_ptr: *mut ffi::FlatbuffersParser, st: &mut SymbolTable) {
    let num_services = ffi::get_num_rpc_services(parser_ptr);
    for i in 0..num_services {
        let def_info = ffi::get_rpc_service_info(parser_ptr, i);
        let Some(name) = c_str_to_optional_string(def_info.name) else {
            continue;
        };

        let namespace: Vec<String> = c_str_to_optional_string(def_info.namespace_)
            .map(|s| s.split('.').map(ToString::to_string).collect())
            .unwrap_or_default();

        let qualified_name = if namespace.is_empty() {
            name.clone()
        } else {
            format!("{}.{}", namespace.join("."), name)
        };

        let file = c_str_to_string(def_info.file);
        let Ok(file_path) = fs::canonicalize(&file) else {
            error!("failed to canonicalize file: {file}");
            continue;
        };

        if st.contains_key(&qualified_name) {
            // This should not happen. The flatbuffers parser returns rich errors for duplicate definitions.
            error!("found duplicate symbol while extracting services: {qualified_name}");
            continue;
        }

        let mut methods = Vec::new();
        let num_methods = ffi::get_num_rpc_methods(parser_ptr, i);
        for j in 0..num_methods {
            let method_info = ffi::get_rpc_method_info(parser_ptr, i, j);

            // Method
            let Some(method_name) = c_str_to_optional_string(method_info.name) else {
                continue;
            };
            let range = Range::new(
                Position::new(
                    method_info.line,
                    method_info.col - as_pos_idx(method_name.chars().count()),
                ),
                Position::new(method_info.line, method_info.col),
            );
            let documentation = c_str_to_optional_string(method_info.documentation);

            // Request
            let Some(request_type_name) = c_str_to_optional_string(method_info.request_type_name)
            else {
                continue;
            };
            let request_range = method_info.request_range.into();
            let Some(request_type) = c_str_to_optional_string(method_info.request_source)
                .and_then(|source| parse_type(&source, request_range))
                .map(|parsed| RpcMethodType {
                    name: request_type_name,
                    parsed,
                    range: request_range,
                })
            else {
                error!("Failed to parse rpc request type at {}:{}:{}. Please open a GitHub Issue: https://github.com/smpanaro/flatbuffers-language-server/issues",
                        file_path.display(), request_range.end.line, request_range.end.character);
                continue;
            };

            // Response
            let Some(response_type_name) = c_str_to_optional_string(method_info.response_type_name)
            else {
                continue;
            };
            let response_range = method_info.response_range.into();
            let Some(response_type) = c_str_to_optional_string(method_info.response_source)
                .and_then(|source| parse_type(&source, response_range))
                .map(|parsed| RpcMethodType {
                    name: response_type_name,
                    parsed,
                    range: response_range,
                })
            else {
                error!("Failed to parse rpc response type at {}:{}:{}. Please open a GitHub Issue: https://github.com/smpanaro/flatbuffers-language-server/issues",
                        file_path.display(), response_range.end.line, response_range.end.character);
                continue;
            };

            methods.push(RpcMethod {
                name: method_name,
                range,
                documentation,
                request_type,
                response_type,
            });
        }

        let symbol_kind = SymbolKind::RpcService(RpcService { methods });
        let documentation = c_str_to_optional_string(def_info.documentation);

        let symbol = create_symbol(
            &file_path,
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
    if !ffi::has_root_type(parser_ptr) {
        return None;
    }
    let root_def = ffi::get_root_type_info(parser_ptr);
    let qualified_name = c_str_to_string(root_def.name);
    let file = c_str_to_string(root_def.file);

    let Ok(file_path) = fs::canonicalize(&file) else {
        error!("failed to canonicalize file: {file}");
        return None;
    };

    let type_source = c_str_to_string(root_def.type_source);
    let type_range = root_def.type_range.into();
    let Some(parsed_type) = parse_type(&type_source, type_range) else {
        error!("Failed to parse root type at {}:{}:{}. Please open a GitHub Issue: https://github.com/smpanaro/flatbuffers-language-server/issues",
            file_path.display(), type_range.end.line, type_range.end.character);
        return None;
    };

    let location = crate::symbol_table::Location {
        path: file_path,
        range: type_range,
    };
    Some(RootTypeInfo {
        location,
        type_name: qualified_name,
        parsed_type,
    })
}

unsafe fn build_include_graph(
    parser_ptr: *mut ffi::FlatbuffersParser,
) -> HashMap<String, Vec<String>> {
    let mut include_graph = HashMap::new();
    let num_files = ffi::get_num_files_with_includes(parser_ptr);
    for i in 0..num_files {
        let Some((original_file_path, canonical_file_path)) = c_str_to_optional_string(
            ffi::get_file_with_includes_path(parser_ptr, i),
        )
        .and_then(|original| {
            fs::canonicalize(&original)
                .ok()
                .map(|canon| (original, canon.to_string_lossy().into_owned()))
        }) else {
            continue;
        };

        let c_file_path = CString::new(original_file_path.clone()).unwrap();
        let num_includes = ffi::get_num_includes_for_file(parser_ptr, c_file_path.as_ptr());
        let mut includes = Vec::new();
        for j in 0..num_includes {
            if let Some(include_path) = c_str_to_optional_string(ffi::get_included_file_path(
                parser_ptr,
                c_file_path.as_ptr(),
                j,
            ))
            .and_then(|p| fs::canonicalize(p).ok())
            .map(|p| p.to_string_lossy().into_owned())
            {
                includes.push(include_path.clone());
            }
        }
        include_graph.insert(canonical_file_path, includes);
    }
    include_graph
}

/// Helper to convert a C string to a Rust String.
unsafe fn c_str_to_string(ptr: *const std::os::raw::c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

/// Helper to convert a C string to an optional Rust String.
unsafe fn c_str_to_optional_string(ptr: *const std::os::raw::c_char) -> Option<String> {
    (!ptr.is_null())
        .then(|| CStr::from_ptr(ptr).to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}

/// Helper to create a symbol and its location.
fn create_symbol(
    file_path: &Path,
    name: String,
    namespace: Vec<String>,
    line: u32,
    col: u32,
    kind: SymbolKind,
    documentation: Option<String>,
) -> Symbol {
    let location = crate::symbol_table::Location {
        path: file_path.to_path_buf(),
        range: Range::new(
            Position::new(line, col - as_pos_idx(name.chars().count())),
            Position::new(line, col),
        ),
    };
    let info = SymbolInfo {
        name,
        namespace,
        location,
        documentation,
        builtin: false,
    };
    Symbol { info, kind }
}
