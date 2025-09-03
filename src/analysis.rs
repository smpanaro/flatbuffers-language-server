use crate::ext::range::RangeExt;
use crate::symbol_table::{self, Symbol};
use crate::utils;
use crate::workspace::Workspace;
use tower_lsp::lsp_types::{Position, Range, Url};

/// Represents what symbol was found at a given location, and what it resolves to.
#[derive(Debug, Clone)]
pub struct ResolvedSymbol {
    /// The symbol that is the ultimate "target" of the hover or go-to-definition.
    pub target: Symbol,
    /// The specific range of the text that was hovered or clicked (e.g., the range of a field's type).
    pub range: Range,
    /// The name of the symbol to use when finding references.
    pub ref_name: String,
}

pub fn resolve_symbol_at(
    workspace: &Workspace,
    uri: &Url,
    position: Position,
) -> Option<ResolvedSymbol> {
    // Check if the cursor is on a root_type declaration
    if let Some(root_type_info) = workspace.root_types.get(uri) {
        if root_type_info.location.range.contains(position) {
            if let Some(target_symbol) = workspace.symbols.get(&root_type_info.type_name) {
                return Some(ResolvedSymbol {
                    target: target_symbol.value().clone(),
                    range: root_type_info.location.range,
                    ref_name: root_type_info.type_name.clone(),
                });
            }
        }
    }

    let symbol_at_cursor = workspace
        .symbols
        .iter()
        .find_map(|entry| entry.value().find_symbol(uri, position).cloned())?;

    if let symbol_table::SymbolKind::Union(u) = &symbol_at_cursor.kind {
        for variant in &u.variants {
            if variant.location.range.contains(position) {
                let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                if let Some(target_symbol) = workspace.symbols.get(base_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.value().clone(),
                        range: variant.location.range,
                        ref_name: base_name.to_string(),
                    });
                // Technically this isn't supported currently.
                } else if let Some(target_symbol) = workspace.builtin_symbols.get(base_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.clone(),
                        range: variant.location.range,
                        ref_name: base_name.to_string(),
                    });
                }
                return None;
            }
        }
    }

    if let symbol_table::SymbolKind::Field(f) = &symbol_at_cursor.kind {
        let inner_type_range =
            utils::type_utils::calculate_inner_type_range(f.type_range, &f.type_name);
        if inner_type_range.contains(position) {
            let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
            if let Some(target_symbol) = workspace.symbols.get(base_type_name) {
                return Some(ResolvedSymbol {
                    target: target_symbol.value().clone(),
                    range: inner_type_range,
                    ref_name: base_type_name.to_string(),
                });
            } else if let Some(target_symbol) = workspace.builtin_symbols.get(base_type_name) {
                return Some(ResolvedSymbol {
                    target: target_symbol.clone(),
                    range: inner_type_range,
                    ref_name: base_type_name.to_string(),
                });
            }
            return None;
        }
    }

    // Default case: the symbol at cursor is the target.
    let range = symbol_at_cursor.info.location.range;
    let ref_name = symbol_at_cursor.info.name.clone();
    Some(ResolvedSymbol {
        target: symbol_at_cursor,
        range,
        ref_name,
    })
}
