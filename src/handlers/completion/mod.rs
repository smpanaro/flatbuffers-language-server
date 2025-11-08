mod attributes;
mod field_type;
mod keyword;
mod root_type;
mod util;

use crate::ext::duration::DurationFormat;
use crate::handlers::completion::field_type::handle_field_type_completion;
use crate::handlers::completion::keyword::handle_keyword_completion;
use crate::handlers::completion::root_type::handle_root_type_completion;
use crate::utils::paths::uri_to_path_buf;
use crate::{
    analysis::WorkspaceSnapshot, handlers::completion::attributes::handle_attribute_completion,
};
use log::debug;
use ropey::Rope;
use std::time::Instant;
use tower_lsp_server::lsp_types::{CompletionParams, CompletionResponse, Position};

pub fn handle_completion(
    snapshot: &WorkspaceSnapshot<'_>,
    params: &CompletionParams,
) -> Option<CompletionResponse> {
    let start = Instant::now();
    let position = params.text_document_position.position;

    let path = uri_to_path_buf(&params.text_document_position.text_document.uri).ok()?;

    let doc = snapshot.documents.get(&path)?;
    let line = doc
        .lines()
        .nth(position.line as usize)
        .map(|s| s.to_string())?;

    if should_suppress_completion(&doc, position) {
        return None;
    }

    let response = if let Some(response) =
        handle_attribute_completion(snapshot, &path, position, &line)
    {
        Some(response)
    } else if let Some(response) = handle_root_type_completion(snapshot, &path, &line, position) {
        Some(response)
    } else if let Some(response) = handle_field_type_completion(snapshot, &path, &line, position) {
        Some(response)
    } else {
        handle_keyword_completion(snapshot, &line)
    };

    let elapsed = start.elapsed();
    debug!(
        "completion in {}: {} L{}C{} -> {} items",
        elapsed.log_str(),
        path.display(),
        position.line + 1,
        position.character + 1,
        response.as_ref().map_or(0, |r| match r {
            CompletionResponse::Array(ref a) => a.len(),
            CompletionResponse::List(ref l) => l.items.len(),
        })
    );

    response
}

fn should_suppress_completion(doc: &Rope, position: Position) -> bool {
    if (position.line as usize) >= doc.len_lines() {
        return false;
    }
    let line = doc.line(position.line as usize);

    // Only suppress on a line that is empty up to the cursor
    if !line
        .slice(0..position.character as usize)
        .to_string()
        .trim()
        .is_empty()
    {
        return false;
    }

    let mut open_braces = 0;
    let mut close_braces = 0;

    // Count braces on previous lines
    for i in 0..position.line {
        // This is safe because we checked position.line < doc.len_lines()
        let prev_line = doc.line(i as usize);
        let line_str = prev_line.to_string();
        // A bit naive, doesn't account for braces in comments or strings.
        // But probably good enough for now.
        open_braces += line_str.matches('{').count();
        close_braces += line_str.matches('}').count();
    }

    // If we have more open than close braces, we are inside a block.
    open_braces > close_braces
}
