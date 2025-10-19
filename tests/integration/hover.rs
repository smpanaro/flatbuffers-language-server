use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use insta::assert_snapshot;
use tower_lsp_server::lsp_types::{
    request, Hover, HoverParams, TextDocumentIdentifier, TextDocumentPositionParams,
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
            work_done_progress_params: Default::default(),
        })
        .await
}

#[tokio::test]
async fn hover_on_keyword() {
    let fixture = r#"
$0table MyTable {
    a: int;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_root_type_keyword() {
    let fixture = r#"
table MyTable { a:int; }
$0root_type MyTable;
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_field_named_table() {
    let fixture = r#"
table MyTable {
    t$0able: int;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_table_definition() {
    let fixture = r#"
table $0MyTable {
    a: int;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_field_primitive_type() {
    let fixture = r#"
table MyTable {
    a: $0int;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
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
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
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
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_field_array_type() {
    let fixture = r#"
struct MyStruct {
    a: [$0int:3];
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
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
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
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
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_on_included_definition() {
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
    let mut harness = TestHarness::new();
    let response = get_hover_response(
        &mut harness,
        main_fixture,
        &[("included.fbs", included_fixture)],
    )
    .await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_mid_type_name() {
    let fixture = r#"
table Widget {
    name: string;
}

table ProductionLine {
    widget: Wid$0get;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_union_namespace() {
    let fixture = r#"
namespace Global;
table Foo {}
table Bar {}
table Baz {}
union Any { Glo$0bal.Foo, Bar, Baz }
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert_snapshot!(serde_json::to_string_pretty(&response).unwrap());
}

#[tokio::test]
async fn hover_despite_warnings() {
    let fixture = r#"
table Tab {
    shouldBeSnakeCase: i$0nt; // flatc will warn that this should be snake_case
}
"#;
    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert!(response.is_some())
}

#[tokio::test]
async fn hover_vector64() {
    // vector64 is parsed uniquely and needs special handling.
    let fixture = r#"
table RootTable {
    big_vector:[ui$0nt8] (vector64);
}
"#;

    let mut harness = TestHarness::new();
    let response = get_hover_response(&mut harness, fixture, &[]).await;
    assert!(response.is_some())
}
