mod attributes;
mod field_type;
mod keyword;
mod root_type;
mod rpc_method;
mod util;

use crate::ext::duration::DurationFormat;
use crate::handlers::completion::field_type::handle_field_type_completion;
use crate::handlers::completion::keyword::handle_keyword_completion;
use crate::handlers::completion::root_type::handle_root_type_completion;
use crate::handlers::completion::rpc_method::handle_rpc_method_completion;
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

    let last_keyword = preceding_symbol_kind(&doc, position);

    let response = if let Some(response) =
        handle_rpc_method_completion(snapshot, &path, &line, position)
            .take_if(|_| last_keyword.as_deref() == Some("rpc_service"))
    {
        Some(response)
    } else if let Some(response) = handle_attribute_completion(snapshot, &path, position, &line)
        .take_if(|_| last_keyword.as_deref() != Some("rpc_service"))
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

// Returns the symbol kind of the first keyword (table, enum, rpc_service) that
// that appears before this position (either on the same line or a prior line).
fn preceding_symbol_kind(doc: &Rope, position: Position) -> Option<String> {
    let mut balance = 0;

    // Iterate backwards from the current line
    for i in (0..=position.line as usize).rev() {
        let line_chunk = doc.line(i);
        let line_str = line_chunk.to_string();

        // Determine the text segment to analyze:
        // - If on the current line, stop at the cursor character.
        // - If on a previous line, analyze the whole line.
        let text_segment = if i == position.line as usize {
            let char_idx = position.character as usize;
            if char_idx < line_str.len() {
                &line_str[..char_idx]
            } else {
                &line_str[..]
            }
        } else {
            &line_str[..]
        };

        // Strip comments (simple check for "//")
        let clean_text = text_segment.split("//").next().unwrap_or("");

        // Scan characters in reverse to check brace balance
        for c in clean_text.chars().rev() {
            match c {
                '}' => balance += 1,
                '{' => balance -= 1,
                _ => {}
            }
        }

        // If balance drops below zero, we found the opening brace for the current context.
        // Check this line for the defining keyword.
        if balance < 0 {
            let trimmed = clean_text.trim();
            if trimmed.contains("rpc_service") {
                return Some("rpc_service".to_string());
            } else if trimmed.contains("table") {
                return Some("table".to_string());
            } else if trimmed.contains("struct") {
                return Some("struct".to_string());
            } else if trimmed.contains("enum") {
                return Some("enum".to_string());
            } else if trimmed.contains("union") {
                return Some("union".to_string());
            }
            // Found a block start, but no recognized keyword (or unmatched brace), stop searching.
            return None;
        }
    }

    None
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
