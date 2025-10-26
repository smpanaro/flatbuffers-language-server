use crate::analysis::workspace_index::WorkspaceIndex;
use crate::ext::range::RangeExt;
use crate::symbol_table::{self, Symbol, SymbolKind};
use crate::utils::paths::uri_to_path_buf;
use dashmap::DashMap;
use ropey::Rope;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLockReadGuard;
use tower_lsp_server::lsp_types::{Position, Range, Uri};

// --- Snapshot Definition ---

pub struct WorkspaceSnapshot<'a> {
    pub index: RwLockReadGuard<'a, WorkspaceIndex>,
    pub documents: Arc<DashMap<PathBuf, Rope>>,
}

impl<'a> Deref for WorkspaceSnapshot<'a> {
    type Target = WorkspaceIndex;

    fn deref(&self) -> &Self::Target {
        &self.index
    }
}

// --- Query API ---

/// Represents what symbol was found at a given location, and what it resolves to.
#[derive(Debug)]
pub struct ResolvedSymbol<'a> {
    /// The symbol that is the ultimate "target" of the hover or go-to-definition.
    pub target: &'a Symbol,
    /// The specific range of the text that was hovered or clicked (e.g., the range of a field's type).
    pub range: Range,
    /// The name of the symbol to use when finding references.
    pub ref_name: String,
}

impl<'a> WorkspaceSnapshot<'a> {
    pub fn resolve_symbol_at(
        &'a self,
        uri: &Uri,
        position: Position,
    ) -> Option<ResolvedSymbol<'a>> {
        // Check if the cursor is on a root_type declaration
        let Ok(path) = uri_to_path_buf(uri) else {
            return None;
        };

        if let Some(root_type_info) = self.root_types.root_types.get(&path) {
            if root_type_info
                .parsed_type
                .type_name
                .range
                .contains(position)
            {
                if let Some(target_symbol) = self.symbols.global.get(&root_type_info.type_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol,
                        range: root_type_info.parsed_type.type_name.range,
                        ref_name: root_type_info.type_name.clone(),
                    });
                }
            }
        }

        let symbol_at_cursor = self
            .symbols
            .global
            .values()
            .find_map(|symbol| symbol.find_symbol(&path, position))?;

        if let symbol_table::SymbolKind::Union(u) = &symbol_at_cursor.kind {
            for variant in &u.variants {
                if !variant.location.range.contains(position) {
                    continue;
                }

                if variant.parsed_type.type_name.range.contains(position) {
                    if let Some(target_symbol) = self.symbols.global.get(&variant.name) {
                        return Some(ResolvedSymbol {
                            target: target_symbol,
                            range: variant.parsed_type.type_name.range,
                            ref_name: variant.name.clone(),
                        });
                    // Technically this isn't supported currently.
                    } else if let Some(target_symbol) = self.symbols.builtins.get(&variant.name) {
                        return Some(ResolvedSymbol {
                            target: target_symbol,
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
                    if let Some(target_symbol) = self.symbols.global.get(&f.type_name) {
                        return Some(ResolvedSymbol {
                            target: target_symbol,
                            range: f.parsed_type.type_name.range,
                            ref_name: f.type_name.clone(),
                        });
                    } else if let Some(target_symbol) = self.symbols.builtins.get(&f.type_name) {
                        return Some(ResolvedSymbol {
                            target: target_symbol,
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
        let qualified_name = symbol_at_cursor.info.qualified_name();
        Some(ResolvedSymbol {
            target: symbol_at_cursor,
            range,
            ref_name: qualified_name,
        })
    }

    pub fn find_enclosing_table(&self, path: &PathBuf, position: Position) -> Option<&Symbol> {
        let mut symbols_before_cursor: Vec<_> = self
            .symbols
            .global
            .values()
            .filter(|symbol| {
                if &symbol.info.location.path != path {
                    return false;
                }
                if symbol.info.location.range.start < position {
                    return true;
                }
                false
            })
            .collect();

        symbols_before_cursor.sort_by_key(|s| s.info.location.range.start);

        if let Some(last_symbol) = symbols_before_cursor.last() {
            if let SymbolKind::Table(_) = &last_symbol.kind {
                return Some(last_symbol);
            }
        }

        None
    }
}
