use std::fs;
use std::sync::Arc;

use flatbuffers_language_server::analysis::Analyzer;
use flatbuffers_language_server::document_store::DocumentStore;
use flatbuffers_language_server::workspace_layout::WorkspaceLayout;
use tempfile::tempdir;

#[tokio::test]
async fn test_analyzer_symbol_index() {
    // 1. Setup the files on disk.
    let dir = tempdir().unwrap();

    /*
    .
    ├── importer.fbs
    ├── no_namespace.fbs
    └── with_namespace.fbs

    1 directory, 3 files
    */

    let no_namespace_fbs_path = dir.path().join("no_namespace.fbs");
    let no_namespace_fbs_content = r#"
        table TableNoNamespace { field: int; }
        struct StructNoNamespace { field: int; }
        enum EnumNoNamespace: byte { A, B }
        union UnionNoNamespace { TableNoNamespace }
    "#;
    fs::write(&no_namespace_fbs_path, no_namespace_fbs_content).unwrap();

    let with_namespace_fbs_path = dir.path().join("with_namespace.fbs");
    let with_namespace_fbs_content = r#"
        namespace MyNamespace;
        table TableWithNamespace { field: int; }
        struct StructWithNamespace { field: int; }
        enum EnumWithNamespace: byte { A, B }
        union UnionWithNamespace { TableWithNamespace }
    "#;
    fs::write(&with_namespace_fbs_path, with_namespace_fbs_content).unwrap();

    let importer_fbs_path = dir.path().join("importer.fbs");
    let importer_fbs_content = r#"
        include "no_namespace.fbs";
        include "with_namespace.fbs";
        table ImporterTable { f1: TableNoNamespace; f2: MyNamespace.TableWithNamespace; }
    "#;
    fs::write(&importer_fbs_path, importer_fbs_content).unwrap();

    // 2. Populate the document store.
    let document_store = DocumentStore::new();
    let canonical_no_namespace_path = fs::canonicalize(&no_namespace_fbs_path).unwrap();
    let canonical_with_namespace_path = fs::canonicalize(&with_namespace_fbs_path).unwrap();
    let canonical_importer_path = fs::canonicalize(&importer_fbs_path).unwrap();

    document_store.document_map.insert(
        canonical_no_namespace_path.clone(),
        no_namespace_fbs_content.into(),
    );
    document_store.document_map.insert(
        canonical_with_namespace_path.clone(),
        with_namespace_fbs_content.into(),
    );
    document_store
        .document_map
        .insert(canonical_importer_path.clone(), importer_fbs_content.into());

    // 3. Make an Analyzer.
    let analyzer = Analyzer::new(Arc::new(document_store));

    // 4. Populate the WorkspaceLayout.
    let mut layout = WorkspaceLayout::new();
    layout.add_root(fs::canonicalize(dir.path()).unwrap());
    let files_to_parse = layout.discover_files();

    // 5. Call parse.
    analyzer.parse(files_to_parse).await;

    // 6. Write assertions against SymbolIndex.
    let snapshot = analyzer.snapshot().await;
    let symbol_index = &snapshot.index.symbols;

    // Assert global map
    assert_eq!(symbol_index.global.len(), 9);
    let expected_global_keys = [
        "TableNoNamespace",
        "StructNoNamespace",
        "EnumNoNamespace",
        "UnionNoNamespace",
        "MyNamespace.TableWithNamespace",
        "MyNamespace.StructWithNamespace",
        "MyNamespace.EnumWithNamespace",
        "MyNamespace.UnionWithNamespace",
        "ImporterTable",
    ];
    for key in expected_global_keys {
        assert!(symbol_index.global.contains_key(key));
    }

    assert_eq!(
        symbol_index
            .global
            .get("TableNoNamespace")
            .unwrap()
            .info
            .location
            .path,
        canonical_no_namespace_path
    );
    assert_eq!(
        symbol_index
            .global
            .get("MyNamespace.TableWithNamespace")
            .unwrap()
            .info
            .location
            .path,
        canonical_with_namespace_path
    );
    assert_eq!(
        symbol_index
            .global
            .get("ImporterTable")
            .unwrap()
            .info
            .location
            .path,
        canonical_importer_path
    );

    // Assert per_file map
    assert_eq!(symbol_index.per_file.len(), 3);
    let no_namespace_symbols = symbol_index
        .per_file
        .get(&canonical_no_namespace_path)
        .unwrap();
    assert_eq!(no_namespace_symbols.len(), 4);
    assert!(no_namespace_symbols.contains(&"TableNoNamespace".to_string()));

    let with_namespace_symbols = symbol_index
        .per_file
        .get(&canonical_with_namespace_path)
        .unwrap();
    assert_eq!(with_namespace_symbols.len(), 4);
    assert!(with_namespace_symbols.contains(&"MyNamespace.TableWithNamespace".to_string()));

    // Note: When this was written, there was no use case for it.
    // Just seemed like the logical behavior.
    let importer_symbols = symbol_index.per_file.get(&canonical_importer_path).unwrap();
    assert_eq!(importer_symbols.len(), 1);
    assert!(importer_symbols.contains(&"ImporterTable".to_string()));
}
