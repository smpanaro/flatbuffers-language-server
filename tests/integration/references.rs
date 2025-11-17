use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use tower_lsp_server::lsp_types::{
    request, Location, PartialResultParams, Position, Range, ReferenceContext, ReferenceParams,
    TextDocumentIdentifier, TextDocumentPositionParams, WorkDoneProgressParams,
};

async fn get_references(fixture: &str, other_files: &[(&str, &str)]) -> Vec<Location> {
    let (content, position) = parse_fixture(fixture);

    let mut workspace = vec![("schema.fbs", content.as_str())];
    workspace.extend_from_slice(other_files);

    let mut harness = TestHarness::new();
    harness.initialize_and_open(&workspace).await;

    let main_file_uri = harness.file_uri("schema.fbs");
    harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_file_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn find_references_for_table() {
    let fixture = r"
namespace MyNS; // otherwise root isn't parsed

table My$0Table {
    a: int;
}

table AnotherTable {
    b: MyTable;
}

root_type MyTable;
";
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
    let fixture = r"
enum MyEnum: byte {
    A$0, B, C
}

table MyTable {
    a: MyEnum = A;
    b: MyEnum = B;
}
";
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
async fn find_references_for_rpc_service() {
    let fixture = r"
namespace Model;

/// Req is a request.
table Req {
    id: string;
}
/// Res is a response.
table Res {
    text: string;
}

namespace API;

/// Service has a comment.
rpc_service Serv$0ice {
    /// Read has a comment.
    Read(Model.Req):Model.Res;
}
";
    let mut locations = get_references(fixture, &[]).await;
    locations.sort_by_key(|loc| loc.range.start.line);

    assert_eq!(locations.len(), 1);

    // Definition
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(15, 12), Position::new(15, 19))
    );
}

#[tokio::test]
async fn find_references_for_rpc_argument() {
    let fixture = r"
namespace Model;

/// Req is a request.
table R$0eq {
    id: string;
}
/// Res is a response.
table Res {
    text: string;
}

namespace API;

/// Service has a comment.
rpc_service Service {
    /// Read has a comment.
    Read(Model.Req):Model.Res;
}
";
    let mut locations = get_references(fixture, &[]).await;
    locations.sort_by_key(|loc| loc.range.start.line);

    assert_eq!(locations.len(), 2);

    // Definition
    assert_eq!(
        locations[0].range,
        Range::new(Position::new(4, 6), Position::new(4, 9))
    );

    // Usage in RPC Method
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(17, 15), Position::new(17, 18))
    );
}

#[tokio::test]
async fn find_references_across_files() {
    let included_fixture = r"
table In$0cludedTable {
    b: bool;
}
";

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

    let included_uri = harness.file_uri("included.fbs");
    let main_uri = harness.file_uri("main.fbs");

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: included_uri.clone(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
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

#[tokio::test]
async fn find_references_respects_namespaces() {
    let coffee_fixture = r"
namespace coffee;

table Be$0an {}
";

    let pastry_fixture = r"
namespace pastry;

table Bean {}
";

    let main_fixture = r#"
include "coffee.fbs";
include "pastry.fbs";

table Beans {
    coffee: coffee.Bean;
    vanilla: pastry.B$0ean;
}
"#;
    let mut harness = TestHarness::new();
    let (coffee_content, coffee_position) = parse_fixture(coffee_fixture);
    let (main_content, pastry_position) = parse_fixture(main_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", &main_content),
            ("coffee.fbs", &coffee_content),
            ("pastry.fbs", pastry_fixture),
        ])
        .await;

    let coffee_uri = harness.file_uri("coffee.fbs");
    let pastry_uri = harness.file_uri("pastry.fbs");
    let main_uri = harness.file_uri("main.fbs");

    // Starting from Declaration.
    let mut coffee_locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: coffee_uri.clone(),
                },
                position: coffee_position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap();

    coffee_locations.sort_by_key(|loc| loc.uri.to_string());

    assert_eq!(coffee_locations.len(), 2);

    // Definition in coffee.fbs
    assert_eq!(coffee_locations[0].uri, coffee_uri);
    assert_eq!(
        coffee_locations[0].range,
        Range::new(Position::new(3, 6), Position::new(3, 10))
    );

    // Usage in main.fbs
    assert_eq!(coffee_locations[1].uri, main_uri);
    assert_eq!(
        coffee_locations[1].range,
        Range::new(Position::new(5, 19), Position::new(5, 23))
    );

    // Starting from Usage.
    let mut pastry_locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position: pastry_position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: ReferenceContext {
                include_declaration: true,
            },
        })
        .await
        .unwrap();

    pastry_locations.sort_by_key(|loc| loc.uri.to_string());

    assert_eq!(pastry_locations.len(), 2);

    // Usage in main.fbs
    assert_eq!(pastry_locations[0].uri, main_uri);
    assert_eq!(
        pastry_locations[0].range,
        Range::new(Position::new(6, 20), Position::new(6, 24))
    );

    // Definition in pastry.fbs
    assert_eq!(pastry_locations[1].uri, pastry_uri);
    assert_eq!(
        pastry_locations[1].range,
        Range::new(Position::new(3, 6), Position::new(3, 10))
    );
}

#[tokio::test]
async fn find_references_respects_nested_namespaces() {
    let included_fixture = r"
namespace One.Two;

table X {}
";

    let main_fixture = r#"
include "included.fbs";
namespace One;

table Y {
    a: Two.$0X;
}
"#;
    let mut harness = TestHarness::new();
    let (main_content, position) = parse_fixture(main_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", &main_content),
            ("included.fbs", included_fixture),
        ])
        .await;

    let included_uri = harness.file_uri("included.fbs");
    let main_uri = harness.file_uri("main.fbs");

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
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
        Range::new(Position::new(3, 6), Position::new(3, 7))
    );

    // Usage in main.fbs
    assert_eq!(locations[1].uri, main_uri);
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(5, 11), Position::new(5, 12))
    );
}

#[tokio::test]
async fn find_references_respects_namespaced_vector() {
    let included_fixture = r"
namespace One.Two;

struct X {
    i: int;
}
";

    let main_fixture = r#"
include "included.fbs";
namespace One;

struct Y {
    a: [Two.$0X:3];
}
"#;
    let mut harness = TestHarness::new();
    let (main_content, position) = parse_fixture(main_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", &main_content),
            ("included.fbs", included_fixture),
        ])
        .await;

    let included_uri = harness.file_uri("included.fbs");
    let main_uri = harness.file_uri("main.fbs");

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
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
        Range::new(Position::new(3, 7), Position::new(3, 8))
    );

    // Usage in main.fbs
    assert_eq!(locations[1].uri, main_uri);
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(5, 12), Position::new(5, 13))
    );
}

#[tokio::test]
async fn find_references_respects_root_type_namespaces() {
    let included_fixture = r"
namespace One.Two;

table Number {}
";

    let main_fixture = r#"
include "included.fbs";
namespace One;

root_type Two.Numbe$0r;
"#;
    let mut harness = TestHarness::new();
    let (main_content, position) = parse_fixture(main_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", &main_content),
            ("included.fbs", included_fixture),
        ])
        .await;

    let included_uri = harness.file_uri("included.fbs");
    let main_uri = harness.file_uri("main.fbs");

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
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
        Range::new(Position::new(3, 6), Position::new(3, 12))
    );

    // Usage in main.fbs
    assert_eq!(locations[1].uri, main_uri);
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(4, 14), Position::new(4, 20))
    );
}

#[tokio::test]
async fn find_references_respects_union_namespaces() {
    let included_fixture = r"
namespace One.Two;

table Number {}
table String {}
";

    let main_fixture = r#"
include "included.fbs";
namespace One;

table Bool {}

union Types {
    Bool,
    One.Two.Number,
    Two.$0String,
}
"#;
    let mut harness = TestHarness::new();
    let (main_content, position) = parse_fixture(main_fixture);

    harness
        .initialize_and_open(&[
            ("main.fbs", &main_content),
            ("included.fbs", included_fixture),
        ])
        .await;

    let included_uri = harness.file_uri("included.fbs");
    let main_uri = harness.file_uri("main.fbs");

    let mut locations = harness
        .call::<request::References>(ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: main_uri.clone(),
                },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
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
        Range::new(Position::new(4, 6), Position::new(4, 12))
    );

    // Usage in main.fbs
    assert_eq!(locations[1].uri, main_uri);
    assert_eq!(
        locations[1].range,
        Range::new(Position::new(9, 8), Position::new(9, 14))
    );
}
