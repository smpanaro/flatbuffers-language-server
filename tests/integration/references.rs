use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use tower_lsp::lsp_types::{
    request, Location, Position, Range, ReferenceContext, ReferenceParams, TextDocumentIdentifier,
    TextDocumentPositionParams,
};

async fn get_references(fixture: &str, other_files: &[(&str, &str)]) -> Vec<Location> {
    let (content, position) = parse_fixture(fixture);

    let mut workspace = vec![("schema.fbs", content.as_str())];
    workspace.extend_from_slice(other_files);

    let mut harness = TestHarness::new();
    harness.initialize_and_open(&workspace).await;

    let main_file_uri = harness.root_uri.join("schema.fbs").unwrap();
    harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_file_uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn find_references_for_table() {
    let fixture = r#"
namespace MyNS; // otherwise root isn't parsed

table My$0Table {
    a: int;
}

table AnotherTable {
    b: MyTable;
}

root_type MyTable;
"#;
    let mut locations = get_references(fixture, &[]).await;
    locations.sort_by_key(|loc| loc.range.start.line);

    assert_eq!(locations.len(), 3);

    // Definition
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(3, 6), Position::new(3, 13))
    );

    // Usage in AnotherTable
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(8, 7), Position::new(8, 14))
    );

    // Usage as root_type
    assert_eq!(
        locations[2].range,
        Range::new(Position::new(11, 10), Position::new(11, 17))
    );
}

#[tokio::test]
#[ignore = "Enum variants are not yet supported for references."]
async fn find_references_for_enum_variant() {
    let fixture = r#"
enum MyEnum: byte {
    A$0, B, C
}

table MyTable {
    a: MyEnum = A;
    b: MyEnum = B;
}
"#;
    let mut locations = get_references(fixture, &[]).await;
    locations.sort_by_key(|loc| loc.range.start.line);

    assert_eq!(locations.len(), 2);

    // Definition
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(2, 4), Position::new(2, 5))
    );

    // Usage in MyTable
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(6, 15), Position::new(6, 16))
    );
}

#[tokio::test]
async fn find_references_across_files() {
    let included_fixture = r#"
table In$0cludedTable {
    b: bool;
}
"#;

    let main_fixture = r#"
include "included.fbs";

table MyTable {
    a: IncludedTable;
}
"#;
    let mut harness = TestHarness::new();
    let (included_content, position) = parse_fixture(included_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", main_fixture),
            ("included.fbs", &included_content),
        ])
        .await;

    let included_uri = harness.root_uri.join("included.fbs").unwrap();
    let main_uri = harness.root_uri.join("main.fbs").unwrap();

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: included_uri.clone(),
                },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap();

    locations.sort_by_key(|loc| loc.uri.to_string());

    assert_eq!(locations.len(), 2);

    // Definition in included.fbs
    assert_eq!(locations[0].uri, included_uri);
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(1, 6), Position::new(1, 19))
    );

    // Usage in main.fbs
    assert_eq!(locations[1].uri, main_uri);
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(4, 7), Position::new(4, 20))
    );
}
