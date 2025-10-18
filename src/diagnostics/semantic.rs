use crate::diagnostics::codes::DiagnosticCode;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, Position, Range};

use crate::symbol_table::{SymbolKind, SymbolTable};

pub fn analyze_deprecated_fields(
    st: &SymbolTable,
    diagnostics: &mut HashMap<PathBuf, Vec<Diagnostic>>,
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
                        .entry(field.info.location.path.to_path_buf())
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
    diagnostics: &mut HashMap<PathBuf, Vec<Diagnostic>>,
    file_contents: &str,
    include_graph: &HashMap<String, Vec<String>>,
    search_paths: &[PathBuf],
    root_type_info: &Option<crate::symbol_table::RootTypeInfo>,
) {
    let mut used_types = HashSet::new();
    if let Some(root_type) = root_type_info {
        if root_type.location.path == st.path {
            used_types.insert(root_type.type_name.clone());
        }
    }

    for symbol in st.values() {
        if symbol.info.location.path != st.path {
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
    for used_type in used_types {
        if let Some(symbol) = st.get(&used_type) {
            let path = &symbol.info.location.path;
            directly_required_files.insert(path.to_path_buf());
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
                if let Some(canonical) = fs::canonicalize(include).ok() {
                    queue.push(canonical.to_path_buf());
                }
            }
        }
    }

    let Some(current_dir) = st.path.parent() else {
        return;
    };

    for (line_num, line) in file_contents.lines().enumerate() {
        if !line.trim().starts_with("include") {
            continue;
        }
        let Some(include_path) = line.split("\"").nth(1) else {
            continue;
        };

        if let Some(absolute_path) = resolve_include(&current_dir, include_path, search_paths) {
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
                    .entry(st.path.clone())
                    .or_default()
                    .push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::HINT),
                        code: Some(tower_lsp::lsp_types::NumberOrString::String(
                            DiagnosticCode::UnusedInclude.as_str().to_string(),
                        )),
                        message: format!("unused include: {}", include_path),
                        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                        ..Default::default()
                    });
            }
        }
    }
}

fn resolve_include(
    current_dir: &Path,
    include_path: &str,
    search_paths: &[PathBuf],
) -> Option<PathBuf> {
    // 1. Check against search paths
    for search_path in search_paths {
        if let Some(canon) = fs::canonicalize(search_path.join(include_path)).ok() {
            if canon.exists() {
                return Some(canon);
            }
        }
    }

    // 2. Check relative to current file's directory
    if let Some(canon) = fs::canonicalize(current_dir.join(include_path)).ok() {
        if canon.exists() {
            return Some(canon);
        }
    }

    None
}
