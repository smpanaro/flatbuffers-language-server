use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, Position, Range, Url};

use crate::symbol_table::{SymbolKind, SymbolTable};

pub fn analyze_deprecated_fields(
    st: &SymbolTable,
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
) {
    for symbol in st.values() {
        let fields = match &symbol.kind {
            SymbolKind::Table(t) => &t.fields,
            SymbolKind::Struct(s) => &s.fields,
            _ => continue,
        };

        for field in fields {
            if let SymbolKind::Field(field_def) = &field.kind {
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

pub fn analyze_unused_includes(
    st: &SymbolTable,
    diagnostics: &mut HashMap<Url, Vec<Diagnostic>>,
    file_contents: &str,
    include_graph: &HashMap<String, Vec<String>>,
    search_paths: &[Url],
    root_type_info: &Option<crate::symbol_table::RootTypeInfo>,
) {
    let mut used_types = HashSet::new();
    if let Some(root_type) = root_type_info {
        if root_type.location.uri == st.uri {
            used_types.insert(root_type.type_name.clone());
        }
    }

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

    let mut directly_required_files = HashSet::new();
    for used_type in &used_types {
        if let Some(symbol) = st.get(used_type) {
            if let Ok(path) = symbol.info.location.uri.to_file_path() {
                directly_required_files.insert(path);
            }
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

    let current_dir = st
        .uri
        .to_file_path()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();

    for (line_num, line) in file_contents.lines().enumerate() {
        if line.starts_with("include") {
            if let Some(start) = line.find('"') {
                if let Some(end) = line.rfind('"') {
                    let include_path_str = &line[start + 1..end];

                    if let Some(absolute_path) =
                        resolve_include(&current_dir, include_path_str, search_paths)
                    {
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
                                    message: format!("unused include: {}", include_path_str),
                                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                    ..Default::default()
                                });
                        }
                    }
                }
            }
        }
    }
}

fn resolve_include(
    current_dir: &Path,
    include_path: &str,
    search_paths: &[Url],
) -> Option<PathBuf> {
    // 1. Check against search paths
    for search_path_url in search_paths {
        if let Ok(search_path) = search_path_url.to_file_path() {
            let mut candidate = search_path;
            candidate.push(include_path);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 2. Check relative to current file's directory
    let mut candidate = current_dir.to_path_buf();
    candidate.push(include_path);
    if candidate.exists() {
        return Some(candidate);
    }

    None
}
