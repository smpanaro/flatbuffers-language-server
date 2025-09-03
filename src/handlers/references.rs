use crate::analysis::resolve_symbol_at;
use crate::server::Backend;
use crate::symbol_table;
use crate::utils;
use log::info;
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

        // Check nested fields in tables and structs
        if let symbol_table::SymbolKind::Table(t) = &symbol.kind {
            for field in &t.fields {
                if let symbol_table::SymbolKind::Field(f) = &field.kind {
                    let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
                    if base_type_name == target_name {
                        let inner_type_range = utils::type_utils::calculate_inner_type_range(
                            f.type_range,
                            &f.type_name,
                        );
                        references.push(Location {
                            uri: file_uri.clone(),
                            range: inner_type_range,
                        });
                    }
                }
            }
        }

        if let symbol_table::SymbolKind::Struct(s) = &symbol.kind {
            for field in &s.fields {
                if let symbol_table::SymbolKind::Field(f) = &field.kind {
                    let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
                    if base_type_name == target_name {
                        let inner_type_range = utils::type_utils::calculate_inner_type_range(
                            f.type_range,
                            &f.type_name,
                        );
                        references.push(Location {
                            uri: file_uri.clone(),
                            range: inner_type_range,
                        });
                    }
                }
            }
        }

        if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
            for variant in &u.variants {
                let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                if base_name == target_name {
                    references.push(Location {
                        uri: file_uri.clone(),
                        range: variant.location.range,
                    });
                }
            }
        }
    }

    // Check for root_type declarations
    for entry in backend.workspace.root_types.iter() {
        let root_type_info = entry.value();
        if root_type_info.type_name == target_name {
            references.push(root_type_info.location.clone());
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
    info!(
        "references in {}ms: {} L{}C{} -> {} refs",
        elapsed.as_millis(),
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
