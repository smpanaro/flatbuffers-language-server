use crate::harness::TestHarness;
use tower_lsp::lsp_types::{notification, Position, Range};

#[tokio::test]
async fn diagnostic_error_has_correct_range() {
    // 1. Define the fixture
    let content = "table MyTable { a: invalid_type; }";

    // 2. Setup the harness and open the file
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    // 3. Wait for the diagnostics notification
    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;

    // 4. Perform programmatic assertions
    assert_eq!(params.uri.path(), "/schema.fbs");
    assert_eq!(
        params.diagnostics.len(),
        1,
        "Expected exactly one diagnostic"
    );

    let diagnostic = &params.diagnostics[0];
    let expected_range = Range::new(Position::new(0, 19), Position::new(0, 31)); // "invalid_type"
    assert_eq!(diagnostic.range, expected_range);
    // Note: We deliberately do NOT assert on `diagnostic.message`.
}

#[tokio::test]
async fn duplicate_enum_definition() {
    let content = r#"
enum MyEnum: byte { A, B }
enum MyEnum: byte { C, D }
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let diagnostics = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &diagnostics.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 5), Position::new(2, 11))
    );
    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 1);
    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(1, 5), Position::new(1, 11))
    );
}

#[tokio::test]
async fn duplicate_enum_variant() {
    let content = "enum MyEnum: byte { A, B, A }";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let diagnostics = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &diagnostics.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(0, 26), Position::new(0, 27))
    );
    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 1);
    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(0, 20), Position::new(0, 21))
    );
}
