use crate::{harness::TestHarness, helpers::parse_fixture};
use insta::assert_snapshot;
use tower_lsp::lsp_types::{
    notification, request, HoverParams, Position, Range, TextDocumentIdentifier,
    TextDocumentPositionParams, VersionedTextDocumentIdentifier,
};

#[tokio::test]
async fn error_appears_on_change_and_is_then_cleared() {
    let initial_content = "table MyTable {}";
    let content_with_error = "table MyTable { a: invalid_type; }";

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", initial_content)])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();

    // 1. We should get an initial empty diagnostic pass.
    {
        let initial_diags = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        assert!(initial_diags.diagnostics.is_empty());
    }

    // 2. Send a change to introduce an error.
    harness
        .change_file(
            VersionedTextDocumentIdentifier {
                uri: schema_uri.clone(),
                version: 2,
            },
            content_with_error,
        )
        .await;

    // 3. Programmatically assert that ONE diagnostic appeared.
    let error_diags = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(error_diags.diagnostics.len(), 1);
    assert_eq!(
        error_diags.diagnostics[0].range,
        Range::new(Position::new(0, 19), Position::new(0, 31))
    );

    // 4. Send a change to fix the error.
    harness
        .change_file(
            VersionedTextDocumentIdentifier {
                uri: schema_uri.clone(),
                version: 3,
            },
            initial_content,
        )
        .await;

    // 5. Snapshot the clearing notification to ensure it's a correctly formed empty list.
    let cleared_diags = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_snapshot!(serde_json::to_string_pretty(&cleared_diags.diagnostics).unwrap());
    // Also add a programmatic check for the most critical part.
    assert!(cleared_diags.diagnostics.is_empty());
}

#[tokio::test]
async fn hover_works_after_file_close() {
    let included_fixture = r#"
// This is from another file.
table IncludedTable {
    b: bool;
}
"#;

    let main_fixture = r#"
include "included.fbs";

table MyTable {
    a: $0IncludedTable;
}
"#;

    let (content, position) = parse_fixture(main_fixture);

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[
            ("schema.fbs", content.as_str()),
            ("included.fbs", included_fixture),
        ])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();
    let included_uri = harness.root_uri.join("included.fbs").unwrap();

    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let initial_response = harness
        .call::<request::HoverRequest>(hover_params.clone())
        .await
        .unwrap();

    harness.close_file(included_uri.clone()).await;

    let post_close_response = harness
        .call::<request::HoverRequest>(hover_params.clone())
        .await
        .unwrap();

    assert_eq!(
        initial_response.range.unwrap(),
        Range::new(Position::new(4, 7), Position::new(4, 20))
    );
    assert_eq!(initial_response, post_close_response);
}

#[tokio::test]
async fn deleted_symbol_causes_diagnostic() {
    let version_one = r#"
table T {}
root_type T;
"#;

    let version_two = r#"
root_type T;
"#;

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", version_one)])
        .await;

    {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        assert_eq!(params.diagnostics.len(), 0);
    }

    let uri = harness.root_uri.join("schema.fbs").unwrap();
    harness
        .change_file(
            VersionedTextDocumentIdentifier { uri, version: 2 },
            version_two,
        )
        .await;

    {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        // This should fail if information about T is incorrectly cached.
        assert_eq!(params.diagnostics.len(), 1);
    }
}
