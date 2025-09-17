use crate::harness::TestHarness;
use insta::assert_snapshot;
use tower_lsp::lsp_types::{notification, Position, Range, VersionedTextDocumentIdentifier};

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
    let initial_diags = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert!(initial_diags.diagnostics.is_empty());

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
async fn diagnostics_are_cleared_on_close() {
    let content_with_error = "table MyTable { a: invalid_type; }";

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content_with_error)])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();

    // 1. We should get a diagnostic for the error.
    let error_diags = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert_eq!(error_diags.diagnostics.len(), 1);
    assert_eq!(error_diags.uri, schema_uri);

    // 2. Close the file.
    harness.close_file(schema_uri.clone()).await;

    // 3. Assert that a clearing diagnostic notification was sent.
    let cleared_diags = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    assert!(cleared_diags.diagnostics.is_empty());
    assert_eq!(cleared_diags.uri, schema_uri);
}
