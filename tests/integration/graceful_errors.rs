use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use flatbuffers_language_server::diagnostics::codes::DiagnosticCode;
use tower_lsp_server::lsp_types::{
    notification, request, Hover, HoverParams, TextDocumentIdentifier, TextDocumentPositionParams,
    WorkDoneProgressParams,
};

async fn get_hover_response(
    harness: &mut TestHarness,
    main_fixture: &str,
    other_files: &[(&str, &str)],
) -> Option<Hover> {
    let (content, position) = parse_fixture(main_fixture);

    let mut workspace = vec![("schema.fbs", content.as_str())];
    workspace.extend_from_slice(other_files);

    harness.initialize_and_open(&workspace).await;

    let main_file_uri = harness.file_uri("schema.fbs");
    harness
        .call::<request::HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_file_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
}

#[tokio::test]
async fn undefined_type_with_metadata() {
    let content = "table GameCharacter { health: NonExistentType (deprecated); }";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;

    assert_eq!(params.diagnostics.len(), 2, "Expected two diagnostics");
    assert!(params
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::UndefinedType.into())));
    assert!(params
        .diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::Deprecated.into())));
}

#[tokio::test]
async fn unused_include_and_undefined_type() {
    let schema_content = "include \"items.fbs\"; table Player { inventory: BogusItem; }";
    let items_content = "namespace items;";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", schema_content), ("items.fbs", items_content)])
        .await;

    let schema_uri = harness.file_uri("schema.fbs");
    let items_uri = harness.file_uri("items.fbs");

    #[allow(clippy::mutable_key_type)]
    let mut all_diagnostics = std::collections::HashMap::new();
    for _ in 0..2 {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        all_diagnostics.insert(params.uri, params.diagnostics);
    }

    let schema_diagnostics = all_diagnostics.get(&schema_uri).unwrap();
    assert_eq!(schema_diagnostics.len(), 2);

    assert!(schema_diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::UnusedInclude.into())));
    assert!(schema_diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::UndefinedType.into())));

    let items_diagnostics = all_diagnostics.get(&items_uri).unwrap();
    assert!(items_diagnostics.is_empty());
}

#[tokio::test]
async fn hover_on_valid_union_with_invalid_table() {
    let fixture = r"
table Monster { health: NotDefined; }
union Enemy { Mo$0nster }
";
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert!(response.is_some(), "Expected hover information for union");
}

#[tokio::test]
async fn hover_with_missing_semicolon() {
    let fixture = r"
table Weapon {}
table Player {
    equipped: Wea$0pon;
    inventory: Weapon
    holding: Weapon;
}
";
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert!(
        response.is_some(),
        "Expected hover information for field before missing semicolon"
    );
}

#[tokio::test]
#[ignore = "Error-tolerant parsing is not implemented."]
async fn hover_on_predeclared_table() {
    let fixture = r"
// Should be able to hover on this pre-declared table
// even though there is a parsing error before it is declared.
union Enemy { Mo$0nster }

table Middle {
    // not_an_attr needs to be pre-declared for flatc to generate code,
    // but this file is perfectly parseable otherwise.
    foo: int (not_an_attr);
}

table Monster {
    name: string;
}
";
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert!(
        response.is_some(),
        "Expected hover information for pre-declared table"
    );
}
