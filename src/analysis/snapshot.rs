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

impl Deref for WorkspaceSnapshot<'_> {
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

    #[must_use] pub fn find_enclosing_table(&self, path: &PathBuf, position: Position) -> Option<&Symbol> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Analyzer;
    use crate::document_store::DocumentStore;
    use crate::utils::paths::path_buf_to_uri;
    use crate::workspace_layout::WorkspaceLayout;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    async fn setup_snapshot(schema: &str) -> (Analyzer, PathBuf, TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.fbs");
        fs::write(&path, schema).unwrap();

        let document_store = DocumentStore::new();
        let canonical_path = fs::canonicalize(&path).unwrap();
        document_store
            .document_map
            .insert(canonical_path.clone(), schema.into());

        let mut layout = WorkspaceLayout::new();
        layout.add_root(fs::canonicalize(dir.path()).unwrap());
        let files_to_parse = layout.discover_files();

        let analyzer = Analyzer::new(Arc::new(document_store));
        analyzer.parse(files_to_parse).await;

        (analyzer, canonical_path, dir)
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_table() {
        let schema = "namespace MyNamespace;\n\ntable MyTable {}\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(2, 8);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyTable");
        assert!(matches!(symbol.target.kind, SymbolKind::Table(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_struct() {
        let schema = "namespace MyNamespace;\n\nstruct MyStruct { x: int; }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(2, 9);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyStruct");
        assert!(matches!(symbol.target.kind, SymbolKind::Struct(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_enum() {
        let schema = "namespace MyNamespace;\n\nenum MyEnum: byte { A, B }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(2, 7);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyEnum");
        assert!(matches!(symbol.target.kind, SymbolKind::Enum(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_union() {
        let schema = "namespace MyNamespace;\n\ntable MyTable {}\nunion MyUnion { MyTable }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(3, 8);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyUnion");
        assert!(matches!(symbol.target.kind, SymbolKind::Union(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_field_type() {
        let schema = "namespace MyNamespace;\n\nstruct MyStruct { x: int; }\ntable MyTable { my_struct: MyStruct; }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(3, 28);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyStruct");
        assert!(matches!(symbol.target.kind, SymbolKind::Struct(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_struct_field() {
        let schema = "namespace MyNamespace;\n\nstruct MyStruct { my_field: int; }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(2, 29);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "int");
        assert!(matches!(symbol.target.kind, SymbolKind::Scalar));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_union_variant() {
        let schema = "namespace MyNamespace;\n\ntable MyTable {}\nunion MyUnion { MyTable }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(3, 18);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyTable");
        assert!(matches!(symbol.target.kind, SymbolKind::Table(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_root_type() {
        let schema = "namespace MyNamespace;\n\ntable MyTable {}\nroot_type MyTable;\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(3, 12);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "MyTable");
        assert!(matches!(symbol.target.kind, SymbolKind::Table(_)));
    }

    #[tokio::test]
    async fn test_resolve_symbol_at_builtin_type() {
        let schema = "namespace MyNamespace;\n\ntable MyTable { my_field: int; }\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let uri = path_buf_to_uri(&path).unwrap();
        let position = Position::new(2, 27);
        let symbol = snapshot.resolve_symbol_at(&uri, position).unwrap();
        assert_eq!(symbol.target.info.name, "int");
        assert!(matches!(symbol.target.kind, SymbolKind::Scalar));
    }

    #[tokio::test]
    async fn test_find_enclosing_table_inside() {
        let schema = "table MyTable {\n  my_field: int;\n}\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let position = Position::new(1, 5);
        let symbol = snapshot.find_enclosing_table(&path, position).unwrap();
        assert_eq!(symbol.info.name, "MyTable");
    }

    #[tokio::test]
    async fn test_find_enclosing_table_outside() {
        let schema = "table MyTable {}\n\nstruct MyStruct {}\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let position = Position::new(2, 5);
        let symbol = snapshot.find_enclosing_table(&path, position);
        assert!(symbol.is_none());
    }

    #[tokio::test]
    async fn test_find_enclosing_table_between() {
        let schema = "table MyTable1 {}\n\ntable MyTable2 {}\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let position = Position::new(1, 5);
        let symbol = snapshot.find_enclosing_table(&path, position).unwrap();
        // TODO: Handle this better (parse brackets?)
        assert_eq!(symbol.info.name, "MyTable1");
    }

    #[tokio::test]
    async fn test_find_enclosing_table_on_definition() {
        let schema = "table MyTable {}\n";
        let (analyzer, path, _dir) = setup_snapshot(schema).await;
        let snapshot = analyzer.snapshot().await;
        let position = Position::new(0, 5);
        let symbol = snapshot.find_enclosing_table(&path, position);
        assert!(symbol.is_none());
    }
}
