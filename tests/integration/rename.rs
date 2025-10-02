use crate::harness::TestHarness;
use crate::helpers::parse_fixture;
use std::collections::HashMap;
use tower_lsp::lsp_types::{
    request, Position, Range, RenameParams, TextDocumentIdentifier, TextDocumentPositionParams,
    TextEdit, Url,
};

async fn get_rename_edits(
    fixture: &str,
    other_files: &[(&str, &str)],
    new_name: &str,
) -> HashMap<Url, Vec<TextEdit>> {
    let (content, position) = parse_fixture(fixture);

    let mut workspace = vec![("schema.fbs", content.as_str())];
    workspace.extend_from_slice(other_files);

    let mut harness = TestHarness::new();
    harness.initialize_and_open(&workspace).await;

    let main_file_uri = harness.root_uri.join("schema.fbs").unwrap();
    let result = harness
        .call::<request::Rename>(RenameParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: main_file_uri },
                position,
            },
            work_done_progress_params: Default::default(),
            new_name: new_name.to_string(),
        })
        .await
        .unwrap();

    result.changes.unwrap_or_default()
}

#[tokio::test]
async fn rename_table_no_namespace() {
    let fixture = r#"
table My$0Table {
    a: int;
}
root_type MyTable;
"#;
    let mut changes = get_rename_edits(fixture, &[], "NewTableName").await;
    assert_eq!(changes.len(), 1);

    let edits = changes.values_mut().next().unwrap();
    edits.sort_by_key(|e| e.range.start.line);

    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits[0],
        TextEdit::new(
            Range::new(Position::new(1, 6), Position::new(1, 13)),
            "NewTableName".to_string()
        )
    );
    assert_eq!(
        edits[1],
        TextEdit::new(
            Range::new(Position::new(4, 10), Position::new(4, 17)),
            "NewTableName".to_string()
        )
    );
}

#[tokio::test]
async fn rename_table_from_usage() {
    let fixture = r#"
table MyTable {
    a: int;
}
root_type My$0Table;
"#;
    let mut changes = get_rename_edits(fixture, &[], "NewTableName").await;
    assert_eq!(changes.len(), 1);

    let edits = changes.values_mut().next().unwrap();
    edits.sort_by_key(|e| e.range.start.line);

    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits[0],
        TextEdit::new(
            Range::new(Position::new(1, 6), Position::new(1, 13)),
            "NewTableName".to_string()
        )
    );
    assert_eq!(
        edits[1],
        TextEdit::new(
            Range::new(Position::new(4, 10), Position::new(4, 17)),
            "NewTableName".to_string()
        )
    );
}

#[tokio::test]
async fn rename_table_with_namespace() {
    let fixture = r#"
namespace My.Api;
table My$0Table {
    a: int;
}
root_type My.Api.MyTable;
"#;
    let mut changes = get_rename_edits(fixture, &[], "NewTableName").await;
    assert_eq!(changes.len(), 1);

    let edits = changes.values_mut().next().unwrap();
    edits.sort_by_key(|e| e.range.start.line);

    assert_eq!(edits.len(), 2);
    assert_eq!(
        edits[0],
        TextEdit::new(
            Range::new(Position::new(2, 6), Position::new(2, 13)),
            "NewTableName".to_string()
        )
    );
    assert_eq!(
        edits[1],
        TextEdit::new(
            Range::new(Position::new(5, 17), Position::new(5, 24)),
            "NewTableName".to_string()
        )
    );
}

#[tokio::test]
async fn rename_across_files() {
    let main_fixture = r#"
include "other.fbs";
root_type Other$0Table;
"#;
    let other_fixture = r#"
table OtherTable {
    a: int;
}
"#;

    let changes = get_rename_edits(main_fixture, &[("other.fbs", other_fixture)], "NewName").await;
    assert_eq!(changes.len(), 2);

    let mut uris: Vec<_> = changes.keys().map(|k| k.to_string()).collect();
    uris.sort();

    assert!(uris[0].ends_with("other.fbs"));
    let other_edits = &changes[&Url::parse(&uris[0]).unwrap()];
    assert_eq!(other_edits.len(), 1);
    assert_eq!(
        other_edits[0],
        TextEdit::new(
            Range::new(Position::new(1, 6), Position::new(1, 16)),
            "NewName".to_string()
        )
    );

    assert!(uris[1].ends_with("schema.fbs"));
    let main_edits = &changes[&Url::parse(&uris[1]).unwrap()];
    assert_eq!(main_edits.len(), 1);
    assert_eq!(
        main_edits[0],
        TextEdit::new(
            Range::new(Position::new(2, 10), Position::new(2, 20)),
            "NewName".to_string()
        )
    );
}
