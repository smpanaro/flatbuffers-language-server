use crate::harness::TestHarness;
use insta::assert_snapshot;
use tower_lsp_server::lsp_types::{request, WorkspaceSymbolParams};

async fn get_workspace_symbols(workspace: &[(&str, &str)], query: &str) -> String {
    let mut harness = TestHarness::new();
    harness.initialize_and_open(workspace).await;

    // Wait for initial diagnostics to be published for all files.
    for _ in 0..workspace.len() {
        harness
            .notification::<tower_lsp_server::lsp_types::notification::PublishDiagnostics>()
            .await;
    }

    let response = harness
        .call::<request::WorkspaceSymbolRequest>(WorkspaceSymbolParams {
            query: query.to_string(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();

    serde_json::to_string_pretty(&response)
        .unwrap()
        .replace(harness.root_uri().as_str(), "[ROOT_URI]")
}

#[tokio::test]
async fn workspace_symbol_returns_all_symbols() {
    let workspace = &[(
        "schema.fbs",
        r#"
table MyTable {
    a: int;
}

struct MyStruct {
    b: bool;
}

enum MyEnum: byte {
    A,
    B,
}

union MyUnion {
    MyTable,
}
"#,
    )];

    let response = get_workspace_symbols(workspace, "").await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn workspace_symbol_fuzzy_match() {
    let workspace = &[(
        "schema.fbs",
        r#"
table MyTable {
    a: int;
}

struct MyStruct {
    b: bool;
}

enum MyEnum: byte {
    A,
    B,
}

union MyUnion {
    MyTable,
}
"#,
    )];

    let response = get_workspace_symbols(workspace, "MyT").await;
    assert_snapshot!(response);
}
