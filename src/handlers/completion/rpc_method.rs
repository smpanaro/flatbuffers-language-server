use std::{path::PathBuf, sync::LazyLock};

use crate::{
    analysis::WorkspaceSnapshot,
    handlers::completion::util::generate_include_text_edit,
    symbol_table::{Symbol, SymbolKind},
    utils::as_pos_idx,
};
use regex::Regex;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemLabelDetails, CompletionResponse, CompletionTextEdit,
    Documentation, MarkupContent, MarkupKind, Position, Range, TextEdit,
};

static REQ_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?<line_prefix>\s*(?<method_name>\w+)+\s*\(\s*)(?<completion_prefix>[\.\w\s]*)$")
        .unwrap()
});

static RESP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?<line_prefix>\s*(?<method_name>\w+)+\s*\(.*\):\s*)(?<completion_prefix>[\.\w\s]*)$",
    )
    .unwrap()
});

pub fn handle_rpc_method_completion(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    line: &str,
    position: Position,
) -> Option<CompletionResponse> {
    let (captures, symbols) = line_completions(snapshot, line, position, &REQ_RE)
        .or_else(|| line_completions(snapshot, line, position, &RESP_RE))?;

    let collisions = snapshot.symbols.collisions();

    let items: Vec<CompletionItem> = symbols
        .into_iter()
        .map(|symbol| {
            // TODO: Dedupe. Heavy overlap with field completion logic.

            let base_name = &symbol.info.name;
            let qualified_name = symbol.info.qualified_name();
            let has_collision = collisions.contains_key(base_name);

            let detail = symbol.info.namespace_str().map_or_else(
                || symbol.type_name().to_string(),
                |ns| format!("{} in {}", symbol.type_name(), ns),
            );

            let use_qualified = captures.completion_prefix.contains('.') || has_collision;
            let new_text = if use_qualified {
                qualified_name.clone()
            } else {
                base_name.clone()
            };

            let (additional_text_edits, preview_text) =
                generate_include_text_edit(snapshot, path, &symbol);

            let sort_priority = i32::from(
                !base_name
                    .to_lowercase()
                    .contains(&captures.method_name.to_lowercase()),
            );
            let sort_text = format!("{sort_priority}_{base_name}");

            CompletionItem {
                label: base_name.clone(),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range: captures.completion_range(position),
                    new_text,
                })),
                additional_text_edits,
                filter_text: Some(qualified_name.clone()),
                sort_text: Some(sort_text),
                kind: Some((&symbol.kind).into()),
                detail: Some(detail),
                label_details: Some(CompletionItemLabelDetails {
                    detail: None, // for function signatures or type annotations, neither of which are relevant for us.
                    description: preview_text.or(symbol.info.namespace_str()), // for fully qualified name or file path.
                }),
                documentation: symbol.info.documentation.as_ref().map(|doc| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: doc.clone(),
                    })
                }),
                ..Default::default()
            }
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

struct LineCaptures {
    line_prefix: String,
    method_name: String,
    completion_prefix: String,
}

impl LineCaptures {
    fn completion_range(&self, cursor: Position) -> Range {
        let start = as_pos_idx(self.line_prefix.chars().count());
        Range::new(
            Position::new(cursor.line, start),
            Position::new(
                cursor.line,
                start + as_pos_idx(self.completion_prefix.chars().count()),
            ),
        )
    }
}

fn line_completions(
    snapshot: &WorkspaceSnapshot,
    line: &str,
    position: Position,
    re: &Regex,
) -> Option<(LineCaptures, Vec<Symbol>)> {
    let line_upto_cursor = &line[..position.character as usize];
    let captures = re.captures(line_upto_cursor).and_then(|capture| {
        let line_prefix = capture.name("line_prefix")?.as_str().to_string();
        let method_name = capture.name("method_name")?.as_str().to_string();
        let completion_prefix = capture.name("completion_prefix")?.as_str().to_string();
        Some(LineCaptures {
            line_prefix,
            method_name,
            completion_prefix,
        })
    })?;

    let symbols = snapshot
        .symbols
        .global
        .values()
        .filter(|&sym| sym.info.name.starts_with(captures.completion_prefix.trim()))
        .filter(|sym| matches!(sym.kind, SymbolKind::Table(_)))
        .cloned()
        .collect();
    Some((captures, symbols))
}
