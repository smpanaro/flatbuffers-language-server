use crate::analysis::WorkspaceSnapshot;
use crate::ext::duration::DurationFormat;
use crate::symbol_table;
use crate::utils::paths::path_buf_to_uri;
use log::debug;
use std::time::Instant;
use tower_lsp_server::lsp_types::{Location, ReferenceParams};

pub fn handle_references(
    snapshot: &WorkspaceSnapshot<'_>,
    params: ReferenceParams,
) -> Option<Vec<Location>> {
    let start = Instant::now();
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let resolved = snapshot.resolve_symbol_at(&uri, position)?;

    if resolved.target.info.builtin {
        return None;
    }

    let target_name = resolved.ref_name;
    let mut references = Vec::new();

    // Find all references to this symbol across all files
    for entry in &snapshot.symbols.global {
        let symbol = entry.1;
        let file_uri = path_buf_to_uri(&symbol.info.location.path).ok()?;

        if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
            for variant in &u.variants {
                if variant.name == target_name {
                    references.push(Location {
                        uri: file_uri.clone(),
                        range: variant.parsed_type.type_name.range,
                    });
                }
            }
        }

        let fields = match &symbol.kind {
            symbol_table::SymbolKind::Table(t) => &t.fields,
            symbol_table::SymbolKind::Struct(s) => &s.fields,
            _ => continue,
        };

        for field in fields {
            if let symbol_table::SymbolKind::Field(f) = &field.kind {
                if f.type_name == target_name {
                    references.push(Location {
                        uri: file_uri.clone(),
                        range: f.parsed_type.type_name.range,
                    });
                }
            }
        }
    }

    // Check for root_type declarations
    for (path, root_type_info) in &snapshot.root_types.root_types {
        let Ok(uri) = path_buf_to_uri(path) else {
            continue;
        };
        if root_type_info.type_name == target_name {
            references.push(Location {
                uri,
                range: root_type_info.parsed_type.type_name.range,
            });
        }
    }

    // Include the definition itself if requested
    if params.context.include_declaration {
        if let Some(def_symbol) = snapshot.symbols.global.get(&target_name) {
            if !def_symbol.info.builtin {
                references.push(def_symbol.info.location.clone().into());
            }
        }
    }

    let elapsed = start.elapsed();
    debug!(
        "references in {}: {} L{}C{} -> {} refs",
        elapsed.log_str(),
        &uri.path(),
        position.line + 1,
        position.character + 1,
        references.len()
    );

    if references.is_empty() {
        None
    } else {
        Some(references)
    }
}
