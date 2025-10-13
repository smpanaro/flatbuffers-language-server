use crate::harness::TestHarness;
use tower_lsp::lsp_types::{
    notification, request, CodeActionContext, CodeActionOrCommand, CodeActionParams,
    DiagnosticSeverity, DiagnosticTag, Position, Range, TextDocumentIdentifier,
};

#[tokio::test]
async fn diagnostic_error_has_correct_range() {
    let content = "table MyTable { a: invalid_type; }";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();
    assert_eq!(params.uri, schema_uri);
    assert_eq!(
        params.diagnostics.len(),
        1,
        "Expected exactly one diagnostic"
    );

    let diagnostic = &params.diagnostics[0];
    let expected_range = Range::new(Position::new(0, 19), Position::new(0, 31)); // "invalid_type"
    assert_eq!(diagnostic.range, expected_range);
}

#[tokio::test]
async fn multiple_files() {
    let content_a = r#"
union Any { Foo }
"#;
    let content_b = r#"
/** Error on a different line. */
union Whichever { One }
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("a.fbs", content_a), ("b.fbs", content_b)])
        .await;

    let a_uri = harness.root_uri.join("a.fbs").unwrap();
    let b_uri = harness.root_uri.join("b.fbs").unwrap();
    let mut params_a = None;
    let mut params_b = None;
    for _ in 0..2 {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == a_uri {
            params_a = Some(params);
        } else if params.uri == b_uri {
            params_b = Some(params);
        } else {
            panic!("unexpected diagnostic: {:?}", params);
        }
    }

    assert_eq!(
        params_a.unwrap().diagnostics[0].range,
        Range::new(Position::new(1, 12), Position::new(1, 15))
    );

    assert_eq!(
        params_b.unwrap().diagnostics[0].range,
        Range::new(Position::new(2, 18), Position::new(2, 21))
    );
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

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
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

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
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

#[tokio::test]
async fn missing_include() {
    let included_content = "enum MyEnum: byte { A, B }";
    let content = r#"
table Foo {
    e: MyEnum;
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content), ("included.fbs", included_content)])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();
    let diagnostics = loop {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == schema_uri {
            break params.diagnostics;
        } else {
            assert_eq!(params.diagnostics.len(), 0)
        }
    };
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics[0].clone();
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 7), Position::new(2, 13))
    );

    // This is quickfix-able.
    let code_actions = harness
        .call::<request::CodeActionRequest>(CodeActionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
            range: Range {
                start: Position::new(2, 7),
                end: Position::new(2, 7),
            },
            context: CodeActionContext {
                diagnostics: vec![diagnostic.clone()],
                ..Default::default()
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await;

    let code_action = match code_actions.unwrap()[0].clone() {
        CodeActionOrCommand::CodeAction(a) => Some(a),
        _ => None,
    }
    .unwrap();

    let changes = code_action
        .edit
        .and_then(|e| e.changes)
        .and_then(|c| c.get(&schema_uri).cloned())
        .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].new_text, "include \"included.fbs\";\n\n");
    assert_eq!(
        changes[0].range,
        Range::new(Position::new(0, 0), Position::new(0, 0))
    );
}

#[tokio::test]
async fn undefined_vector_type() {
    let content = "table Foo { bar: [Baz]; }";
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(0, 18), Position::new(0, 21)),
        "range should exclude vector brackets"
    );
}

#[tokio::test]
async fn deprecated_field() {
    let content = r#"
table Foo {
    f: [int];
    depr: int (deprecated);
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(3, 4), Position::new(3, u32::MAX)),
    );
    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::HINT));
    // This tag is more appropriate for flatbuffers' usage of deprecation.
    assert_eq!(diagnostic.tags, Some(vec![DiagnosticTag::UNNECESSARY]))
}

#[tokio::test]
async fn missing_semicolon_include() {
    let content = r#"
include "coffee.fbs"
include "pastries.fbs";
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[
            ("schema.fbs", content),
            ("coffee.fbs", "namespace coffee;"),
            ("pastries.fbs", "namespace pastries;"),
        ])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();
    let diagnostics = loop {
        let param = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if param.uri == schema_uri {
            break param.diagnostics;
        } else {
            assert!(param.diagnostics.is_empty());
        }
    };
    assert_eq!(diagnostics.len(), 1);

    let diagnostic = &diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(1, 20), Position::new(1, 21)),
    );
    assert_eq!(diagnostic.message, "expected `;`, found `include`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(2, 0), Position::new(2, 7)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // the second "include"
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(1, 20), Position::new(1, 21)),
    );
    assert_eq!(related_information[1].message, "add `;` here");
}

#[tokio::test]
async fn missing_semicolon_field() {
    let content = r#"
table Coffee {
    roast: string

    origin: string;
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 17), Position::new(2, 18)),
    );
    assert_eq!(diagnostic.message, "expected `;`, found `origin`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(4, 4), Position::new(4, 10)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // "origin"
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(2, 17), Position::new(2, 18)),
    );
    assert_eq!(related_information[1].message, "add `;` here");
}

#[tokio::test]
async fn missing_semicolon_end_of_file() {
    let content = r#"
table Coffee {}

root_type Coffee
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(3, 16), Position::new(3, 17)),
    );
    assert_eq!(diagnostic.message, "expected `;`, found `end of file`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 1);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(3, 16), Position::new(3, 17)),
    );
    assert_eq!(related_information[0].message, "add `;` here");
}

#[tokio::test]
async fn missing_semicolon_comment() {
    let content = r#"
table ids {
    one: int (id: 0)
    // two: int (id: 1);
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 20), Position::new(2, 21)),
    );
    assert_eq!(diagnostic.message, "expected `;`, found `}`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(4, 0), Position::new(4, 1)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // the closing brace
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(2, 20), Position::new(2, 21)),
    );
    assert_eq!(related_information[1].message, "add `;` here");
}

#[tokio::test]
async fn expecting_bracket() {
    let content = r#"
table Foo {
    foo: [int;
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 13), Position::new(2, 14)),
    );
    assert_eq!(diagnostic.message, "expected `]`, found `;`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(2, 13), Position::new(2, 14)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // ";"
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(2, 13), Position::new(2, 14)),
    );
    assert_eq!(related_information[1].message, "add `]` here");
}

#[tokio::test]
async fn expecting_bracket_no_semicolon() {
    let content = r#"
table Foo {
    foo: [int
}
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(2, 13), Position::new(2, 14)),
    );
    assert_eq!(diagnostic.message, "expected `]`, found `}`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(3, 0), Position::new(3, 1)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // "}"
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(2, 13), Position::new(2, 14)),
    );
    assert_eq!(related_information[1].message, "add `]` here");
}

#[tokio::test]
async fn expecting_table_brace() {
    let content = r#"
table Foo
    foo: int;
"#;
    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(1, 9), Position::new(1, 10)),
    );
    assert_eq!(diagnostic.message, "expected `{`, found `foo`");

    let related_information = diagnostic.related_information.as_ref().unwrap();
    assert_eq!(related_information.len(), 2);

    assert_eq!(
        related_information[0].location.range,
        Range::new(Position::new(2, 4), Position::new(2, 7)),
    );
    assert_eq!(
        related_information[0].message,
        "unexpected token" // "foo"
    );

    assert_eq!(
        related_information[1].location.range,
        Range::new(Position::new(1, 9), Position::new(1, 10)),
    );
    assert_eq!(related_information[1].message, "add `{` here");
}

#[tokio::test]
async fn field_case_warning() {
    let content = r#"
table MyTable { furryWombat:string; }"#;

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", content)])
        .await;

    let params = harness
        .notification::<notification::PublishDiagnostics>()
        .await;
    let diagnostic = &params.diagnostics[0];
    assert_eq!(
        diagnostic.range,
        Range::new(Position::new(1, 16), Position::new(1, 27))
    );
    assert_eq!(
        diagnostic.message,
        "field `furryWombat` should be in snake_case e.g. `furry_wombat`"
    );
    assert_eq!(diagnostic.severity, Some(DiagnosticSeverity::WARNING));
}

#[tokio::test]
async fn undefined_type_in_included_file() {
    let included = r#"
table Pen {}

table Ink {
    brand: Brand; // undefined
}
"#;
    let main = r#"
include "included.fbs";
root_type Pen;
"#;

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", main), ("included.fbs", included)])
        .await;

    let included_uri = harness.root_uri.join("included.fbs").unwrap();
    let mut diagnostics = vec![];
    let mut other_diagnostics_count = 0;
    for _ in 0..2 {
        let param = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if param.uri == included_uri {
            diagnostics.push(param.diagnostics);
        } else {
            // schema.fbs itself has no errors.
            assert!(param.diagnostics.is_empty());
            other_diagnostics_count += 1;
        }
    }
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(other_diagnostics_count, 1);

    for d in diagnostics {
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].range.start.character, 11);
        assert_eq!(d[0].range.end.character, 16);
    }
}

#[tokio::test]
async fn undefined_vector_type_in_included_file() {
    let included = r#"
table Pen {}

table Ink {
    brand: [Brand]; // undefined
}
"#;
    let main = r#"
include "included.fbs";
root_type Pen;
"#;

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", main), ("included.fbs", included)])
        .await;

    let included_uri = harness.root_uri.join("included.fbs").unwrap();
    let mut diagnostics = vec![];
    let mut other_diagnostics_count = 0;
    for _ in 0..2 {
        let param = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if param.uri == included_uri {
            diagnostics.push(param.diagnostics);
        } else {
            // schema.fbs itself has no errors.
            assert!(param.diagnostics.is_empty());
            other_diagnostics_count += 1;
        }
    }
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(other_diagnostics_count, 1);

    for d in diagnostics {
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].range.start.character, 12);
        assert_eq!(d[0].range.end.character, 17);
    }
}

#[tokio::test]
async fn no_unused_include_namespace() {
    let schema_fixture = r#"
include "../related/other.fbs";

table MyTable {
    a: N.OtherTable;
}
"#;
    let other_fixture = "namespace N; table OtherTable {}";

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[
            ("related/other.fbs", other_fixture),
            ("core/schema.fbs", schema_fixture),
        ])
        .await;

    for _ in 0..2 {
        let param = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        assert!(param.diagnostics.is_empty());
    }
}
