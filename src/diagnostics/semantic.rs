use crate::diagnostics::codes::DiagnosticCode;
use crate::utils::as_pos_idx;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};

use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, Position, Range};

use crate::symbol_table::{RootTypeInfo, SymbolKind, SymbolTable};

pub fn analyze_deprecated_fields<S: BuildHasher>(
    st: &SymbolTable,
    diagnostics: &mut HashMap<PathBuf, Vec<Diagnostic>, S>,
) {
    for symbol in st.values() {
        if symbol.info.location.path != st.path {
            continue;
        }

        let fields = match &symbol.kind {
            SymbolKind::Table(t) => &t.fields,
            SymbolKind::Struct(s) => &s.fields,
            _ => continue,
        };

        for field in fields {
            if let SymbolKind::Field(field_def) = &field.kind {
                if field_def.deprecated {
                    diagnostics
                        .entry(field.info.location.path.clone())
                        .or_default()
                        .push(Diagnostic {
                            range: Range {
                                start: field.info.location.range.start,
                                end: Position {
                                    line: field.info.location.range.end.line,
                                    character: u32::MAX,
                                },
                            },
                            code: Some(DiagnosticCode::Deprecated.into()),
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

struct IncludeStatement {
    canonical: PathBuf,
    /// text inside the quoted string
    text: String,
    line: u32,
    line_length: u32,
}

pub fn analyze_unused_includes<S: BuildHasher>(
    st: &SymbolTable,
    diagnostics: &mut HashMap<PathBuf, Vec<Diagnostic>, S>,
    file_contents: &str,
    include_graph: &HashMap<String, Vec<String>, S>,
    search_paths: &[PathBuf],
    root_type_info: &Option<RootTypeInfo>,
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
            _ => (),
        }
    }

    // Need to get from the file's includes to each of these.
    let mut symbol_defining_files = HashSet::new();
    for used_type in &used_types {
        if let Some(symbol) = st.get(used_type) {
            let path = &symbol.info.location.path;
            // TODO: Make everything PathBuf.
            if let Some(path_str) = path.to_str() {
                symbol_defining_files.insert(path_str);
                log::info!("{} requires {}", symbol.info.name, path.display());
            }
        }
    }

    let Some(current_dir) = st.path.parent() else {
        return;
    };

    // Need to do this because although we know what files are imported,
    // we don't know what lines those imports are on.
    let include_statements: Vec<_> = file_contents
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("include"))
        .filter_map(|(idx, line)| line.split('"').nth(1).map(|path| (idx, line, path))) // contents inside the quotes
        .filter_map(|(idx, line, path)| {
            resolve_include(current_dir, path, search_paths)
                .map(|abs_path| (idx, line, path, abs_path))
        })
        .map(|(idx, line, path, abs_path)| IncludeStatement {
            canonical: abs_path,
            text: path.to_string(),
            line: as_pos_idx(idx),
            line_length: as_pos_idx(line.len()),
        })
        .collect();

    let file_to_transitive_includes = transitive_include_graph(include_graph);
    for include in include_statements {
        let provides_transitively: HashSet<_> = file_to_transitive_includes
            .get(include.canonical.to_str().unwrap_or_default())
            .map(|transitive_includes| {
                transitive_includes
                    .intersection(&symbol_defining_files)
                    .collect()
            })
            .unwrap_or_default();

        let provides_directly =
            symbol_defining_files.contains(include.canonical.to_str().unwrap_or_default());
        if provides_directly || !provides_transitively.is_empty() {
            continue;
        }

        let line = include.line;
        let range = Range {
            start: Position::new(line, 0),
            end: Position::new(line, include.line_length),
        };
        diagnostics
            .entry(st.path.clone())
            .or_default()
            .push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::HINT),
                code: Some(DiagnosticCode::UnusedInclude.into()),
                message: format!("unused include: {}", include.text),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                ..Default::default()
            });
    }
}

fn resolve_include(
    current_dir: &Path,
    include_path: &str,
    search_paths: &[PathBuf],
) -> Option<PathBuf> {
    // 1. Check against search paths
    for search_path in search_paths {
        if let Ok(canon) = fs::canonicalize(search_path.join(include_path)) {
            if canon.exists() {
                return Some(canon);
            }
        }
    }

    // 2. Check relative to current file's directory
    if let Ok(canon) = fs::canonicalize(current_dir.join(include_path)) {
        if canon.exists() {
            return Some(canon);
        }
    }

    None
}

fn transitive_include_graph<S: BuildHasher>(
    direct_include_graph: &HashMap<String, Vec<String>, S>,
) -> HashMap<&str, HashSet<&str>> {
    fn dfs<'a, S: BuildHasher>(
        node: &'a str,
        graph: &'a HashMap<String, Vec<String>, S>,
        visited: &mut HashSet<&'a str>,
    ) {
        if let Some(neighbors) = graph.get(node) {
            for n in neighbors {
                if visited.insert(n) {
                    dfs(n, graph, visited);
                }
            }
        }
    }

    let mut result = HashMap::new();
    for key in direct_include_graph.keys() {
        let mut visited = HashSet::new();
        dfs(key, direct_include_graph, &mut visited);
        result.insert(key.as_str(), visited);
    }
    result
}
