use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use flatbuffers_language_server::analysis::Analyzer;
use flatbuffers_language_server::document_store::DocumentStore;
use flatbuffers_language_server::utils::paths::path_buf_to_uri;
use tempfile::tempdir;
use tower_lsp_server::lsp_types::{FileChangeType, FileEvent};

#[tokio::test]
async fn test_analyzer_workspace_manipulations() {
    // 1. Initial Setup
    let dir = tempdir().unwrap();

    let mut a_root_dir = dir.path().join("a-root");
    fs::create_dir_all(&a_root_dir).unwrap();
    a_root_dir = fs::canonicalize(a_root_dir).unwrap();

    let first_file_path = a_root_dir.join("a.fbs");
    let first_file_content = r#"
        #include "../a-root/a.fbs";
        table A { field: int; }
        root_type A;
    "#;
    fs::write(&first_file_path, first_file_content).unwrap();

    let document_store = DocumentStore::new();
    let analyzer = Analyzer::new(Arc::new(document_store));

    // 2. Add root and parse
    analyzer
        .handle_workspace_folder_changes(vec![path_buf_to_uri(&a_root_dir).unwrap()], vec![])
        .await;

    assert_matches_fresh_analyzer(Some(a_root_dir.clone()), &analyzer).await;

    // 3. Remove root
    analyzer
        .handle_workspace_folder_changes(vec![], vec![path_buf_to_uri(&a_root_dir).unwrap()])
        .await;

    assert_matches_fresh_analyzer(None, &analyzer).await;

    // 3.5 Add new root
    let mut b_root_dir = dir.path().join("b-root");
    fs::create_dir_all(&b_root_dir).unwrap();
    b_root_dir = fs::canonicalize(b_root_dir).unwrap();

    analyzer
        .handle_workspace_folder_changes(vec![path_buf_to_uri(&b_root_dir).unwrap()], vec![])
        .await;

    assert_matches_fresh_analyzer(Some(b_root_dir.clone()), &analyzer).await;

    // 4. Add new file and parse
    let second_file_path = b_root_dir.join("b.fbs");
    let second_file_content = "table B { field: int; };\nroot_type B;";
    fs::write(&second_file_path, second_file_content).unwrap();
    let canonical_file_b_path = fs::canonicalize(&second_file_path).unwrap();
    analyzer
        .handle_file_changes(vec![FileEvent {
            uri: path_buf_to_uri(&canonical_file_b_path).unwrap(),
            typ: FileChangeType::CREATED,
        }])
        .await;

    assert_matches_fresh_analyzer(Some(b_root_dir.clone()), &analyzer).await;

    // 5. Remove file
    fs::remove_file(&second_file_path).unwrap();
    analyzer
        .handle_file_changes(vec![FileEvent {
            uri: path_buf_to_uri(&canonical_file_b_path).unwrap(),
            typ: FileChangeType::DELETED,
        }])
        .await;

    assert_matches_fresh_analyzer(Some(b_root_dir.clone()), &analyzer).await;

    // 6. Remove second root
    analyzer
        .handle_workspace_folder_changes(vec![], vec![path_buf_to_uri(&b_root_dir).unwrap()])
        .await;
    assert_matches_fresh_analyzer(None, &analyzer).await;
}

async fn assert_matches_fresh_analyzer(root_dir: Option<PathBuf>, mutated_analyzer: &Analyzer) {
    let fresh_analyzer = Analyzer::new(Arc::new(DocumentStore::new()));
    if let Some(root_dir) = root_dir {
        fresh_analyzer
            .handle_workspace_folder_changes(vec![path_buf_to_uri(&root_dir).unwrap()], vec![])
            .await;
    }

    let fresh_index = fresh_analyzer.snapshot().await.index;
    let mutated_index = mutated_analyzer.snapshot().await.index;
    assert_eq!(mutated_index.diagnostics, fresh_index.diagnostics);
    assert_eq!(mutated_index.dependencies, fresh_index.dependencies);
    assert_eq!(mutated_index.root_types, fresh_index.root_types);
    assert_eq!(mutated_index.symbols.global, fresh_index.symbols.global);
    assert_eq!(mutated_index.symbols.per_file, fresh_index.symbols.per_file);
}
