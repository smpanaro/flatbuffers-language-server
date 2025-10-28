use std::fs;
use std::sync::Arc;

use flatbuffers_language_server::analysis::Analyzer;
use flatbuffers_language_server::document_store::DocumentStore;
use flatbuffers_language_server::workspace_layout::WorkspaceLayout;
use tempfile::tempdir;

#[tokio::test]
async fn test_analyzer_root_type_store() {
    // 1. Setup the files on disk.
    let dir = tempdir().unwrap();

    /*
    .
    ├── other.fbs
    ├── root.fbs
    └── schemas
        └── common.fbs

    2 directories, 3 files
    */

    let schemas_dir = dir.path().join("schemas");
    fs::create_dir(&schemas_dir).unwrap();

    let common_fbs_path = schemas_dir.join("common.fbs");
    let common_fbs_content = r#"
        namespace schemas;
        table CommonObject { value:int; }
    "#;
    fs::write(&common_fbs_path, common_fbs_content).unwrap();

    let root_fbs_path = dir.path().join("root.fbs");
    let root_fbs_content = r#"
        include "schemas/common.fbs";
        table MyRoot { common:schemas.CommonObject; }
        root_type MyRoot;
    "#;
    fs::write(&root_fbs_path, root_fbs_content).unwrap();

    let other_fbs_path = dir.path().join("other.fbs");
    let other_fbs_content = r#"
        table OtherRoot { field:string; }
        root_type OtherRoot;
    "#;
    fs::write(&other_fbs_path, other_fbs_content).unwrap();

    // 2. Populate the document store.
    let document_store = DocumentStore::new();
    let canonical_common_path = fs::canonicalize(&common_fbs_path).unwrap();
    let canonical_root_path = fs::canonicalize(&root_fbs_path).unwrap();
    let canonical_other_path = fs::canonicalize(&other_fbs_path).unwrap();

    document_store
        .document_map
        .insert(canonical_common_path.clone(), common_fbs_content.into());
    document_store
        .document_map
        .insert(canonical_root_path.clone(), root_fbs_content.into());
    document_store
        .document_map
        .insert(canonical_other_path.clone(), other_fbs_content.into());

    // 3. Make an Analyzer.
    let analyzer = Analyzer::new(Arc::new(document_store));

    // 4. Populate the WorkspaceLayout.
    let mut layout = WorkspaceLayout::new();
    layout.add_root(fs::canonicalize(dir.path()).unwrap());
    let files_to_parse = layout.discover_files();

    // 5. Call parse.
    analyzer.parse(files_to_parse).await;

    // 6. Write assertions against RootTypeStore.
    let snapshot = analyzer.snapshot().await;
    let root_type_store = &snapshot.index.root_types.root_types;

    assert_eq!(root_type_store.len(), 2);

    let root_file_root_type = root_type_store.get(&canonical_root_path).unwrap();
    assert_eq!(root_file_root_type.type_name, "MyRoot");
    assert_eq!(root_file_root_type.parsed_type.type_name.text, "MyRoot");

    let other_file_root_type = root_type_store.get(&canonical_other_path).unwrap();
    assert_eq!(other_file_root_type.type_name, "OtherRoot");
    assert_eq!(other_file_root_type.parsed_type.type_name.text, "OtherRoot");

    assert!(root_type_store.get(&canonical_common_path).is_none());
}
