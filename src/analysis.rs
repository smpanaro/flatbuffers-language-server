use crate::ext::range::RangeExt;
use crate::symbol_table::{self, Symbol};
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
        if root_type_info
            .parsed_type
            .type_name
            .range
            .contains(position)
        {
            if let Some(target_symbol) = workspace.symbols.get(&root_type_info.type_name) {
                return Some(ResolvedSymbol {
                    target: target_symbol.value().clone(),
                    range: root_type_info.parsed_type.type_name.range,
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
            if !variant.location.range.contains(position) {
                continue;
            }

            if variant.parsed_type.type_name.range.contains(position) {
                if let Some(target_symbol) = workspace.symbols.get(&variant.name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.value().clone(),
                        range: variant.parsed_type.type_name.range,
                        ref_name: variant.name.clone(),
                    });
                // Technically this isn't supported currently.
                } else if let Some(target_symbol) = workspace.builtin_symbols.get(&variant.name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.clone(),
                        range: variant.parsed_type.type_name.range,
                        ref_name: variant.name.clone(),
                    });
                }
            }
            return None;
        }
    }

    if let symbol_table::SymbolKind::Field(f) = &symbol_at_cursor.kind {
        if f.type_range.contains(position) {
            // Check if the cursor is on one of the namespace parts
            for part in &f.parsed_type.namespace {
                if part.range.contains(position) {
                    // TODO: Add support for go-to-definition on namespace parts
                    return None;
                }
            }

            // Check if the cursor is on the type name
            if f.parsed_type.type_name.range.contains(position) {
                if let Some(target_symbol) = workspace.symbols.get(&f.type_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.value().clone(),
                        range: f.parsed_type.type_name.range,
                        ref_name: f.type_name.clone(),
                    });
                } else if let Some(target_symbol) = workspace.builtin_symbols.get(&f.type_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.clone(),
                        range: f.parsed_type.type_name.range,
                        ref_name: f.type_name.clone(),
                    });
                }
            }

            return None;
        }
    }

    // Default case: the symbol at cursor is the target.
    let range = symbol_at_cursor.info.location.range;
    let qualified_name = if symbol_at_cursor.info.namespace.is_empty() {
        symbol_at_cursor.info.name.clone()
    } else {
        format!(
            "{}.{}",
            symbol_at_cursor.info.namespace.join("."),
            symbol_at_cursor.info.name
        )
    };
    Some(ResolvedSymbol {
        target: symbol_at_cursor,
        range,
        ref_name: qualified_name,
    })
}

pub fn find_enclosing_table<'a>(
    workspace: &'a Workspace,
    uri: &Url,
    position: Position,
) -> Option<Symbol> {
    let mut symbols_before_cursor: Vec<_> = workspace
        .symbols
        .iter()
        .filter(|entry| {
            let symbol = entry.value();
            if symbol.info.location.uri != *uri {
                return false;
            }
            if symbol.info.location.range.start < position {
                return true;
            }
            false
        })
        .map(|entry| entry.value().clone())
        .collect();

    symbols_before_cursor.sort_by_key(|s| s.info.location.range.start);

    if let Some(last_symbol) = symbols_before_cursor.last() {
        if let symbol_table::SymbolKind::Table(_) = &last_symbol.kind {
            return Some(last_symbol.clone());
        }
    }

    None
}
