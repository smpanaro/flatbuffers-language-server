use std::fs;
use std::sync::Arc;

use flatbuffers_language_server::analysis::Analyzer;
use flatbuffers_language_server::document_store::DocumentStore;
use flatbuffers_language_server::utils::paths::uri_to_path_buf;
use flatbuffers_language_server::workspace_layout::WorkspaceLayout;
use tempfile::tempdir;

#[tokio::test]
async fn test_analyzer_diagnostic_store() {
    // 1. Setup the files on disk.
    let dir = tempdir().unwrap();

    /*
    .
    ├── invalid.fbs
    └── valid.fbs

    1 directory, 2 files
    */

    let valid_fbs_path = dir.path().join("valid.fbs");
    let valid_fbs_content = "table MyTable { field: int; }";
    fs::write(&valid_fbs_path, valid_fbs_content).unwrap();

    let invalid_fbs_path = dir.path().join("invalid.fbs");
    let invalid_fbs_content = "table MyOtherTable { field: UndefinedType; }";
    fs::write(&invalid_fbs_path, invalid_fbs_content).unwrap();

    // 2. Populate the document store.
    let document_store = DocumentStore::new();
    let canonical_valid_path = fs::canonicalize(&valid_fbs_path).unwrap();
    let canonical_invalid_path = fs::canonicalize(&invalid_fbs_path).unwrap();

    document_store
        .document_map
        .insert(canonical_valid_path.clone(), valid_fbs_content.into());
    document_store
        .document_map
        .insert(canonical_invalid_path.clone(), invalid_fbs_content.into());

    // 3. Make an Analyzer.
    let analyzer = Analyzer::new(Arc::new(document_store));

    // 4. Populate the WorkspaceLayout.
    let mut layout = WorkspaceLayout::new();
    layout.add_root(fs::canonicalize(dir.path()).unwrap());
    let files_to_parse = layout.discover_files();

    // 5. Call parse.
    let diagnostics = analyzer.parse(files_to_parse).await;

    // 6. Assert that the contents of diagnostics returned by parse match our expectations.
    assert_eq!(diagnostics.len(), 2);

    let (invalid_url, invalid_diags) = diagnostics
        .iter()
        .find(|(url, _)| url.as_str().ends_with("/invalid.fbs"))
        .unwrap();
    let invalid_path = uri_to_path_buf(invalid_url).unwrap();
    assert_eq!(invalid_path, canonical_invalid_path);
    assert_eq!(invalid_diags.len(), 1);
    assert!(invalid_diags[0].message.contains("UndefinedType"));

    let (valid_url, valid_diags) = diagnostics
        .iter()
        .find(|(url, _)| url.as_str().ends_with("/valid.fbs"))
        .unwrap();
    let valid_path = uri_to_path_buf(valid_url).unwrap();
    assert_eq!(valid_path, canonical_valid_path);
    assert!(valid_diags.is_empty());
}
