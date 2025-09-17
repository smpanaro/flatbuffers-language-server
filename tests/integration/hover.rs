use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use insta::assert_snapshot;
use tower_lsp::lsp_types::{
    request, HoverParams, TextDocumentIdentifier, TextDocumentPositionParams, Url,
};

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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
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
table Widget {
    name: string;
}

table ProductionLine {
    widget: $0Widget;
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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_field_vector_type() {
    let fixture = r#"
/// A 2D coordinate.
struct Point {
    x: float;
    y: float;
}

table Line {
    points: [$0Point];
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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_field_array_type() {
    let fixture = r#"
struct MyStruct {
    a: [$0int:3];
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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_union_member() {
    let fixture = r#"
/// A table with b.
table MyTable {
    b: bool;
}

union MyUnion {
    $0MyTable
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
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}

#[tokio::test]
async fn hover_on_root_type() {
    let fixture = r#"
namespace MyNS; // root type requires a namespac

table MyTable {
    b: bool;
}

root_type $0MyTable;
"#;
    let (content, position) = parse_fixture(fixture);

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", &content)])
        .await;

    let res = harness
        .call::<request::HoverRequest>(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path("/schema.fbs").unwrap(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await;

    assert_snapshot!(serde_json::to_string_pretty(&res).unwrap());
}
