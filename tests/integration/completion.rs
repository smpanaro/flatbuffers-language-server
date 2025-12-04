use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use flatbuffers_language_server::ext::all_diagnostics::AllDiagnostics;
use insta::assert_snapshot;
use tower_lsp_server::lsp_types::{
    notification, request, CompletionContext, CompletionParams, CompletionTriggerKind,
    PartialResultParams, TextDocumentIdentifier, TextDocumentPositionParams,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};

async fn get_completion_list(
    harness: &mut TestHarness,
    main_fixture: &str,
    other_files: &[(&str, &str)],
) -> String {
    let (final_content, position) = parse_fixture(main_fixture);

    let cursor_line = position.line as usize;
    let initial_content: String = final_content
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != cursor_line)
        .map(|(_, line)| line)
        .collect::<Vec<_>>()
        .join("\n");

    let mut initial_workspace = vec![("schema.fbs", initial_content.as_str())];
    initial_workspace.extend_from_slice(other_files);
    harness.initialize_and_open(&initial_workspace).await;

    let main_file_uri = harness.file_uri("schema.fbs");

    // Wait for initial diagnostics to be published for all files.
    for _ in 0..initial_workspace.len() {
        let diags = harness
            .notification::<notification::PublishDiagnostics>()
            .await;
        // Catch unrelated errors (e.g. struct with no fields) that prevent
        // the initial symbols from being loaded.
        assert_eq!(diags.diagnostics.len(), 0, "unexpected diagnostics");
    }
    assert_eq!(
        harness.call::<AllDiagnostics>(()).await.len(),
        initial_workspace.len()
    );

    harness
        .change_file_sync(
            VersionedTextDocumentIdentifier {
                uri: main_file_uri.clone(),
                version: 2,
            },
            &final_content,
        )
        .await;

    let response = harness
        .call::<request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_file_uri },
                position,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::INVOKED,
                trigger_character: None,
            }),
        })
        .await;

    let mut items = response
        .map(|resp| match resp {
            tower_lsp_server::lsp_types::CompletionResponse::Array(items) => items,
            tower_lsp_server::lsp_types::CompletionResponse::List(list) => list.items,
        })
        .unwrap_or_default();
    items.sort_by_key(|item| item.sort_text.as_ref().unwrap_or(&item.label).to_owned());

    let completion_labels: Vec<String> = items.into_iter().map(|item| item.label).collect();

    serde_json::to_string_pretty(&completion_labels).unwrap()
}

#[tokio::test]
async fn completion_for_type_in_field_name() {
    let fixture = r"
// Naive alpha sort would place this first.
table Abacus {}

// This should come first since the field name contains this type.
table Widget {
    a: int;
}

table Collection {
    primaryWidget: $0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_root_type() {
    let fixture = r"
namespace MyNamespace;

table MyTable {}
table AnotherTable {}
struct RootTypeCannotBeStruct { a: int; }

root_type $0
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_keywords() {
    let fixture = r"
table T {}
t$0
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn no_completion_on_new_line_in_table_block() {
    let fixture = r"
table MyTable {
    $0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn no_completion_on_new_line_in_struct_block() {
    let fixture = r"
struct MyStruct {
    a: int;
    $0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_includes_all_primitive_types() {
    let fixture = r"
table MyTable {
    a: $0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_field_type_prefix() {
    let fixture = r"
table MyTable {
    a: u$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_from_included_file() {
    let included_fixture = r"
/// A table from another file.
table IncludedTable {}
struct IncludedStruct {
    must_have_a_field: float;
}
";

    let main_fixture = r"
table MyTable {
    a: In$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(
        &mut harness,
        main_fixture,
        &[("included.fbs", included_fixture)],
    )
    .await;
    assert_snapshot!(response);
}

#[tokio::test]
#[ignore = "Table attribute completions are not supported."]
async fn completion_for_attribute_on_table() {
    let fixture = r"
table MyTable($0) {}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
#[ignore = "Table attribute completions are not supported."]
async fn completion_for_filtered_attribute_on_table() {
    let fixture = r"
table MyTable(h$0) {}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_attribute_on_field() {
    let fixture = r"
table MyTable {
    my_field: int ($0);
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_filtered_attribute_on_field() {
    let fixture = r"
table MyTable {
    my_field: int (k$0);
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_partial_attribute_on_field() {
    let fixture = r"
table MyTable {
    my_field: int ($0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_partial_filtered_attribute_on_field() {
    let fixture = r"
table MyTable {
    my_field: int (k$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_second_field_id_attribute() {
    let fixture = r"
table FieldType {}
table MyTable {
    first_field: FieldType (id: 0, required);
    second_field: int (i$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_second_attribute() {
    let fixture = r"
table MyTable {
    first_field: int (id: 0, $0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_enum_variant_attribute() {
    let fixture = r"
enum MyEnum : ushort {
    A,
    B ($0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_attribute_outside_parens() {
    let fixture = r"
enum MyEnum : ushort {
    A,
    B (custom),$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_field_namespace_partial() {
    let fixture = r"
namespace one.two.three;

table Tree {}

table Forest {
    oak: on$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_field_namespace_partial_with_dot() {
    let fixture = r"
namespace one.two.three;

table Tree {}

table Forest {
    oak: one.two.$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_field_namespace_partial_with_dot_no_type() {
    let fixture = r"
namespace one.two.three;

table Tree {}

table Forest {
    oak: one.two.three.$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_field_namespace_partial_with_dot_and_type_part() {
    let fixture = r"
namespace one.two.three;

table Tree {}

table Forest {
    oak: one.two.three.T$0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}

#[tokio::test]
async fn completion_for_rpc_service_request() {
    let fixture = r"
table ReqOne {}
table ReqTwo {}
struct StructsNotAllowed { f: int; }

rpc_service Service {
    Read(ReqOne): ReqOne;
    Write($0
}
";
    let mut harness = TestHarness::new();
    let response = get_completion_list(&mut harness, fixture, &[]).await;
    assert_snapshot!(response);
}
