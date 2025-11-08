use crate::{harness::TestHarness, helpers::parse_fixture};
use flatbuffers_language_server::diagnostics::codes::DiagnosticCode;
use insta::assert_snapshot;
use tower_lsp_server::lsp_types::{
    notification, request, DiagnosticSeverity, HoverParams, Position, Range,
    TextDocumentIdentifier, TextDocumentPositionParams, VersionedTextDocumentIdentifier,
    WorkDoneProgressParams,
};

#[tokio::test]
async fn error_appears_on_change_and_is_then_cleared() {
    let initial_content = "table MyTable {}";
    let content_with_error = "table MyTable { a: invalid_type; }";

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", initial_content)])
        .await;

    let schema_uri = harness.file_uri("schema.fbs");

    // 1. We should get an initial empty diagnostic pass.
    {
        let initial_diags = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        assert!(initial_diags.diagnostics.is_empty());
    }

    // 2. Send a change to introduce an error.
    harness
        .change_file_sync(
            VersionedTextDocumentIdentifier::new(schema_uri.clone(), 2),
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
        .change_file_sync(
            VersionedTextDocumentIdentifier::new(schema_uri.clone(), 3),
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
    let included_fixture = r"
// This is from another file.
table IncludedTable {
    b: bool;
}
";

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

    let schema_uri = harness.file_uri("schema.fbs");
    let included_uri = harness.file_uri("included.fbs");

    let hover_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams::new(
            TextDocumentIdentifier::new(schema_uri),
            position,
        ),
        work_done_progress_params: WorkDoneProgressParams::default(),
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
    let version_one = r"
table T {}
root_type T;
";

    let version_two = r"
root_type T;
";

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

    let uri = harness.file_uri("schema.fbs");
    harness
        .change_file_sync(VersionedTextDocumentIdentifier::new(uri, 2), version_two)
        .await;

    {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        // This should fail if information about T is incorrectly cached.
        assert_eq!(params.diagnostics.len(), 1);
    }
}

#[tokio::test]
async fn saving_included_file_clears_diagnostic() {
    let including = r#"
include "included.fbs"; // unused include initially

table T { i: I; } // Diagnostic here: I is referenced but not defined
"#;

    let included_before = r"
table X {}
";

    let included_after = r"
table I {} // Change so that I is now defined.
";

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open_some(
            &[
                ("including.fbs", including),
                ("included.fbs", included_before),
            ],
            &[],
        )
        .await;

    let including_uri = harness.file_uri("including.fbs");
    let diagnostics = loop {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == including_uri {
            break params.diagnostics;
        }
        assert_eq!(params.diagnostics.len(), 0);
    };

    // Diagnostic in includer.fbs since I is not found.
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::UnusedInclude.into())));
    assert!(diagnostics
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::UndefinedType.into())));

    let included_uri = harness.file_uri("included.fbs");
    harness
        .save_file(TextDocumentIdentifier::new(included_uri), included_after)
        .await;

    let diagnostics = loop {
        let params = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        if params.uri == including_uri {
            break params.diagnostics;
        }
        assert_eq!(params.diagnostics.len(), 0);
    };

    // No more diagnostic now that I is defined.
    assert_eq!(diagnostics.len(), 0);
}

#[tokio::test]
async fn saving_included_file_with_error() {
    let including = r#"
include "included.fbs";

table Foo { e: WithError; }

table Other {
}
// ^ The last bracket of this file will report the error for included.
"#;

    let included_original = r"
table AnotherTable {
    to: int;
    make: int;
    this: int;
    file: int;
    longer: int;
    than: int;
    includer: int;
}

table WithError {
    f: int;
}
";
    let included_error = included_original.replace("f: int;", "f: int");

    let mut harness = TestHarness::new();
    let included_uri = harness.file_uri("included.fbs");
    let including_uri = harness.file_uri("including.fbs");

    {
        // Initial open.
        harness
            .initialize_and_open_some(
                &[
                    ("including.fbs", including),
                    ("included.fbs", included_original),
                ],
                &["included.fbs"],
            )
            .await;

        for _ in 0..2 {
            let params = harness
                .notification::<notification::PublishDiagnostics>()
                .await;
            assert_eq!(params.diagnostics.len(), 0);
        }
    }

    {
        // Change to introduce an error.
        harness
            .change_file_sync(
                VersionedTextDocumentIdentifier::new(included_uri.clone(), 1),
                &included_error,
            )
            .await;

        let diagnostics = loop {
            let params = harness
                .notification::<notification::PublishDiagnostics>()
                .await;
            if params.uri == included_uri {
                break params.diagnostics;
            }
            assert_eq!(params.diagnostics.len(), 0);
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].range.start, Position::new(12, 10));
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ExpectingToken.into())
        );
    }

    {
        // Save with the error (and trigger reparsing of includer).
        harness
            .save_file_sync(
                TextDocumentIdentifier::new(included_uri.clone()),
                &included_error,
            )
            .await;

        let diagnostics = loop {
            let params = harness
                .notification::<notification::PublishDiagnostics>()
                .await;
            if params.uri == including_uri {
                break params.diagnostics;
            }
            assert_eq!(params.diagnostics.len(), 0);
        };
        // Parsing error in WithError makes the include appear unused.
        // TODO: This could be improved with a syntax-tolerant parser.
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::UnusedInclude.into())
        );
    }
}

#[tokio::test]
async fn saving_included_file_maintains_hints() {
    let including = r#"
include "included.fbs";
union Any { T }
"#;

    let included = r#"
include "including.fbs"; // unused

table T {
    depr: int (deprecated);
}
"#;

    let mut harness = TestHarness::new();
    let included_uri = harness.file_uri("included.fbs");

    {
        // Initial open.
        harness
            .initialize_and_open_some(
                &[("including.fbs", including), ("included.fbs", included)],
                &["included.fbs"],
            )
            .await;

        for _ in 0..2 {
            let params = harness
                .notification::<notification::PublishDiagnostics>()
                .await;
            if included_uri == params.uri {
                assert_eq!(params.diagnostics.len(), 2); // unused + deprecated
                assert!(params
                    .diagnostics
                    .iter()
                    .all(|d| d.severity == Some(DiagnosticSeverity::HINT)));
            } else {
                assert_eq!(params.diagnostics.len(), 0);
            }
        }
    }

    {
        // Save to trigger reparsing of included + includer.
        // Use the test-only synchronous save so that any new diagnostics
        // are published before the sync response is received.
        // Otherwise we don't have a non-timeout based way of knowing
        // that no diagnostics were published.
        harness
            .save_file_sync(TextDocumentIdentifier::new(included_uri.clone()), included)
            .await;
        let notifs = harness.pending_notifications::<notification::PublishDiagnostics>();
        assert!(notifs.is_empty());
    }
}
