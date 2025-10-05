use crate::analysis::resolve_symbol_at;
use crate::ext::duration::DurationFormat;
use crate::server::Backend;
use crate::symbol_table;
use log::debug;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{Location, ReferenceParams};

pub async fn handle_references(
    backend: &Backend,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    let start = Instant::now();
    let uri = params.text_document_position.text_document.uri;
    let position = params.text_document_position.position;

    let Some(resolved) = resolve_symbol_at(&backend.workspace, &uri, position) else {
        return Ok(None);
    };

    if resolved.target.info.location.uri.scheme() == "builtin" {
        return Ok(None);
    }

    let target_name = resolved.ref_name;
    let mut references = Vec::new();

    // Find all references to this symbol across all files
    for entry in backend.workspace.symbols.iter() {
        let symbol = entry.value();
        let file_uri = &symbol.info.location.uri;

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
    for entry in backend.workspace.root_types.iter() {
        let root_type_info = entry.value();
        if root_type_info.type_name == target_name {
            references.push(Location {
                uri: entry.key().clone(),
                range: root_type_info.parsed_type.type_name.range,
            });
        }
    }

    // Include the definition itself if requested
    if params.context.include_declaration {
        if let Some(def_symbol) = backend.workspace.symbols.get(&target_name) {
            if def_symbol.info.location.uri.scheme() != "builtin" {
                references.push(def_symbol.info.location.clone());
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

    Ok(if references.is_empty() {
        None
    } else {
        Some(references)
    })
}
