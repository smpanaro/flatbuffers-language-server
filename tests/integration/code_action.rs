use crate::harness::TestHarness;
use insta::assert_snapshot;
use tower_lsp::lsp_types::{request, CodeActionContext, CodeActionParams, TextDocumentIdentifier};

/// Gets code actions for a multi-file workspace, waiting for a specific diagnostic to appear first.
async fn get_code_actions_for_workspace(
    harness: &mut TestHarness,
    workspace: &[(&str, &str)],
    file_to_test: &str,
    diagnostic_message: &str,
) -> String {
    harness.initialize_and_open(workspace).await;

    let file_uri = harness.root_uri.join(file_to_test).unwrap();

    // Wait for the specific diagnostic we want to test.
    let diagnostic = harness
        .wait_for_diagnostic(diagnostic_message)
        .await
        .unwrap_or_else(|| {
            panic!(
                "Did not receive expected diagnostic: {}",
                diagnostic_message
            )
        });

    let response = harness
        .call::<request::CodeActionRequest>(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: file_uri },
            range: diagnostic.range,
            context: CodeActionContext {
                diagnostics: vec![diagnostic],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await;

    // Sort actions by title for stable snapshots.
    let mut actions = response.unwrap_or_default();
    actions.sort_by(|a, b| match (a, b) {
        (
            tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(a),
            tower_lsp::lsp_types::CodeActionOrCommand::CodeAction(b),
        ) => a.title.cmp(&b.title),
        _ => std::cmp::Ordering::Equal,
    });

    serde_json::to_string_pretty(&actions).unwrap()
}

#[tokio::test]
async fn remove_unused_include() {
    let schema_fixture = r#"
include "other.fbs"; // This is unused.
include "another.fbs";

table MyTable {
    a: AnotherTable;
}
"#;
    let another_fixture = "table AnotherTable {}";
    let other_fixture = "table UnusedTable {}";

    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("schema.fbs", schema_fixture),
            ("another.fbs", another_fixture),
            ("other.fbs", other_fixture),
        ],
        "schema.fbs",
        "unused include: other.fbs",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn import_undefined_type() {
    let definition_fixture = r#"namespace MyNamespace;
table MyTable {}
"#;
    let schema_fixture = r#"table T {
    f: MyTable;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("definitions.fbs", definition_fixture),
            ("schema.fbs", schema_fixture),
        ],
        "schema.fbs",
        "type referenced but not defined",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn import_undefined_type_matching_namespace() {
    let definition_fixture = r#"namespace MyNamespace;

table MyTable {}
"#;
    let schema_fixture = r#"namespace MyNamespace;

table T {
    f: MyTable;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("definitions.fbs", definition_fixture),
            ("schema.fbs", schema_fixture),
        ],
        "schema.fbs",
        "type referenced but not defined",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn import_qualified_undefined_type() {
    let definition_fixture = r#"namespace MyNamespace;

table MyTable {}
"#;
    let schema_fixture = r#"table T {
    f: MyNamespace.MyTable;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("definitions.fbs", definition_fixture),
            ("schema.fbs", schema_fixture),
        ],
        "schema.fbs",
        "type referenced but not defined",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn import_undefined_type_with_existing_namespace() {
    let definition_fixture = r#"namespace MyNamespace;

table MyTable {}
"#;
    let schema_fixture = r#"namespace OtherNamespace;

table T {
    f: MyTable;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("definitions.fbs", definition_fixture),
            ("schema.fbs", schema_fixture),
        ],
        "schema.fbs",
        "type referenced but not defined",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn import_undefined_type_already_included() {
    let definition_fixture = r#"namespace MyNamespace;

table MyTable {}
"#;
    let schema_fixture = r#"include "definitions.fbs";

table T {
    f: MyTable;
}
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("definitions.fbs", definition_fixture),
            ("schema.fbs", schema_fixture),
        ],
        "schema.fbs",
        "type referenced but not defined",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn no_code_action_for_other_diagnostics() {
    let fixture = "table T { a: }"; // Invalid syntax

    let mut harness = TestHarness::new();
    harness
        .initialize_and_open(&[("schema.fbs", fixture)])
        .await;

    let schema_uri = harness.root_uri.join("schema.fbs").unwrap();
    let diagnostic = harness.get_first_diagnostic_for_file(&schema_uri).await;

    let response = harness
        .call::<request::CodeActionRequest>(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: schema_uri },
            range: diagnostic.range,
            context: CodeActionContext {
                diagnostics: vec![diagnostic],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await;

    let actions = response.unwrap_or_default();
    assert!(actions.is_empty());
    let response_str = serde_json::to_string_pretty(&actions).unwrap();
    let redacted_response = response_str.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn expecting_token_quickfix() {
    let schema_fixture = r#"
table Foo {
    foo: [int;
}
"#;

    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[("schema.fbs", schema_fixture)],
        "schema.fbs",
        "expected `]`, found `;`",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn missing_semicolon_quickfix() {
    let schema_fixture = r#"
table MyTable {}
root_type MyTable"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[("schema.fbs", schema_fixture)],
        "schema.fbs",
        "expected `;`, found `end of file`",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}

#[tokio::test]
async fn missing_semicolon_include_quickfix() {
    let schema_fixture = r#"
include "coffee.fbs"
include "pastries.fbs";
"#;
    let mut harness = TestHarness::new();
    let response = get_code_actions_for_workspace(
        &mut harness,
        &[
            ("schema.fbs", schema_fixture),
            ("coffee.fbs", "namespace coffee;"),
            ("pastries.fbs", "namespace pastries;"),
        ],
        "schema.fbs",
        "expected `;`, found `include`",
    )
    .await;

    let redacted_response = response.replace(&harness.root_uri.to_string(), "[ROOT_URI]/");
    assert_snapshot!(redacted_response);
}
