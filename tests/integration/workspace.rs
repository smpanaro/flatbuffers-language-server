use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use tower_lsp_server::lsp_types::{
    notification::{self, DidChangeWatchedFiles, DidChangeWorkspaceFolders},
    request, CompletionContext, CompletionParams, CompletionTriggerKind,
    DidChangeWatchedFilesParams, DidChangeWorkspaceFoldersParams, FileChangeType, FileEvent,
    TextDocumentIdentifier, TextDocumentPositionParams, WorkspaceFolder,
    WorkspaceFoldersChangeEvent,
};
use tower_lsp_server::UriExt;

#[tokio::test]
async fn diagnostics_are_cleared_on_file_deletion() {
    let content = "table MyTable { a: invalid_type; }";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let schema_uri = harness.file_uri("schema.fbs");

    // Wait for initial diagnostic
    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, schema_uri);
    assert_eq!(params.diagnostics.len(), 1);

    // Simulate file deletion
    let file_path = schema_uri.to_file_path().unwrap();
    std::fs::remove_file(file_path).unwrap();
    harness
        .send_notification::<DidChangeWatchedFiles>(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: schema_uri.clone(),
                typ: FileChangeType::DELETED,
            }],
        })
        .await;

    // We expect a new "publishDiagnostics" notification with an empty list of diagnostics.
    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, schema_uri);
    assert!(params.diagnostics.is_empty());
}

#[tokio::test]
async fn completions_are_removed_on_file_deletion() {
    let file_to_delete = "table TypeFromDeletedFile {}";
    let (file_with_completion, position) = parse_fixture(
        r#"
table T {
    f: TypeFr$0
}"#,
    );

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[
            ("file_to_delete.fbs", file_to_delete),
            ("file_with_completion.fbs", &file_with_completion),
        ])
        .await;

    // Wait for initial diagnostics to clear.
    for _ in 0..2 {
        harness
            .notification::<notification::PublishDiagnostics>()
            .await;
    }

    let completion_uri = harness.file_uri("file_with_completion.fbs");

    let completions = harness
        .call::<request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: completion_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await
        .unwrap();

    let labels = match completions {
        tower_lsp_server::lsp_types::CompletionResponse::Array(items) => {
            items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
        tower_lsp_server::lsp_types::CompletionResponse::List(list) => {
            list.items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
    };

    assert!(labels.contains(&"TypeFromDeletedFile".to_string()));

    // Simulate file deletion
    let deleted_uri = harness.file_uri("file_to_delete.fbs");
    let deleted_path = deleted_uri.to_file_path().unwrap();
    std::fs::remove_file(deleted_path).unwrap();
    harness
        .send_notification::<DidChangeWatchedFiles>(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: deleted_uri.clone(),
                typ: FileChangeType::DELETED,
            }],
        })
        .await;

    // Wait for the diagnostic publish for the deleted file to be cleared
    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, deleted_uri);
    assert!(params.diagnostics.is_empty());

    let completions = harness
        .call::<request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: completion_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await
        .unwrap();

    let labels = match completions {
        tower_lsp_server::lsp_types::CompletionResponse::Array(items) => {
            items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
        tower_lsp_server::lsp_types::CompletionResponse::List(list) => {
            list.items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
    };

    assert!(!labels.contains(&"TypeFromDeletedFile".to_string()));
}

#[tokio::test]
async fn diagnostics_are_cleared_on_workspace_folder_removal() {
    let mut harness = TestHarness::new();
    let folders = vec!["root1", "root2"];
    let files = vec![("root2/schema.fbs", "table T { f: S; }")];
    harness
        .initialize_with_workspace_folders(&folders, &files, &["root2/schema.fbs"])
        .await;

    let schema_uri = harness.file_uri("root2/schema.fbs");

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, schema_uri);
    assert_eq!(params.diagnostics.len(), 1);

    let root2_uri = harness.file_uri("root2/");
    let removed_folder = WorkspaceFolder {
        uri: root2_uri.clone(),
        name: "root2".to_string(),
    };

    harness
        .send_notification::<DidChangeWorkspaceFolders>(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![],
                removed: vec![removed_folder],
            },
        })
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, schema_uri);
    assert!(params.diagnostics.is_empty());
}

#[tokio::test]
async fn completions_are_removed_on_workspace_folder_removal() {
    let mut harness = TestHarness::new();
    let folders = vec!["root1", "root2"];
    let (file_with_completion, position) = parse_fixture(
        r#"
table T {
    f: TypeFr$0
}"#,
    );
    let files = vec![
        ("root1/completion.fbs", file_with_completion.as_str()),
        ("root2/schema.fbs", "table TypeFromRemovedFile {}"),
    ];
    harness
        .initialize_with_workspace_folders(&folders, &files, &["root1/completion.fbs"])
        .await;

    let completion_uri = harness.file_uri("root1/completion.fbs");
    for _ in 0..2 {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == completion_uri {
            assert_eq!(params.diagnostics.len(), 1);
        } else {
            assert!(params.diagnostics.is_empty());
        }
    }

    let completions = harness
        .call::<request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: completion_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await
        .unwrap();
    let labels = match completions {
        tower_lsp_server::lsp_types::CompletionResponse::Array(items) => {
            items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
        tower_lsp_server::lsp_types::CompletionResponse::List(list) => {
            list.items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
    };
    assert!(labels.contains(&"TypeFromRemovedFile".to_string()));

    let root2_uri = harness.file_uri("root2/");
    let removed_folder = WorkspaceFolder {
        uri: root2_uri.clone(),
        name: "root2".to_string(),
    };

    harness
        .send_notification::<DidChangeWorkspaceFolders>(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![],
                removed: vec![removed_folder],
            },
        })
        .await;

    // The server should emit an empty diagnostic for the file in the removed folder.
    // Note: Technically since this is the same as the diagnostic that was emitted
    //       initially, it would be okay to omit it here too.
    let removed_file_uri = harness.file_uri("root2/schema.fbs");
    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(params.uri, removed_file_uri);
    assert!(params.diagnostics.is_empty());

    let completions = harness
        .call::<request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: completion_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await
        .unwrap();
    let labels = match completions {
        tower_lsp_server::lsp_types::CompletionResponse::Array(items) => {
            items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
        tower_lsp_server::lsp_types::CompletionResponse::List(list) => {
            list.items.into_iter().map(|i| i.label).collect::<Vec<_>>()
        }
    };
    assert!(!labels.contains(&"TypeFromRemovedFile".to_string()));
}
