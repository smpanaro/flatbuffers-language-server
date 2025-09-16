use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use insta::assert_snapshot;
use tower_lsp::lsp_types::{request, HoverParams, TextDocumentPositionParams};

#[tokio::test]
async fn hover_on_table_definition() {
    let fixture = r#"
table $0MyTable {
    a: int;
}
"#;
    let (content, position) = parse_fixture(fixture);

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", &content)])
        .await;

    let res = harness
        .call::<request::HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: tower_lsp::lsp_types::TextDocumentIdentifier {
                    uri: tower_lsp::lsp_types::Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_field_primitive_type() {
    let fixture = r#"
table MyTable {
    a: $0int;
}
"#;
    let (content, position) = parse_fixture(fixture);

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", &content)])
        .await;

    let res = harness
        .call::<request::HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: tower_lsp::lsp_types::TextDocumentIdentifier {
                    uri: tower_lsp::lsp_types::Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_field_table_type() {
    let fixture = r#"
table MyTable1 {
    b: bool;
}

table MyTable2 {
    a: $0MyTable1;
}
"#;
    let (content, position) = parse_fixture(fixture);

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", &content)])
        .await;

    let res = harness
        .call::<request::HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: tower_lsp::lsp_types::TextDocumentIdentifier {
                    uri: tower_lsp::lsp_types::Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}
