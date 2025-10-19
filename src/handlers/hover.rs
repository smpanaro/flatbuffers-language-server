use crate::analysis::resolve_symbol_at;
use crate::ext::duration::DurationFormat;
use crate::server::Backend;
use crate::utils::paths::uri_to_path_buf;
use log::debug;
use ropey::Rope;
use std::time::Instant;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    Hover, HoverContents, HoverParams, MarkupContent, MarkupKind, Position, Range,
};

fn find_word_at_pos(line: &str, char_pos: u32) -> (usize, usize) {
    let char_pos = char_pos as usize;
    let start = line[..char_pos]
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(0, |i| i + 1);
    let end = line[char_pos..]
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .map_or(line.len(), |i| char_pos + i);
    (start, end)
}

fn is_inside_braces(doc: &Rope, position: Position) -> bool {
    let mut open_braces = 0;
    let mut close_braces = 0;

    // Count braces on previous lines
    for i in 0..position.line {
        if let Some(line) = doc.lines().nth(i as usize) {
            let line_str = line.to_string();
            open_braces += line_str.matches('{').count();
            close_braces += line_str.matches('}').count();
        }
    }

    // Count braces on current line up to cursor
    if let Some(line) = doc.lines().nth(position.line as usize) {
        let line_str = line.to_string();
        let line_before_cursor = &line_str[..position.character as usize];
        open_braces += line_before_cursor.matches('{').count();
        close_braces += line_before_cursor.matches('}').count();
    }

    // If we have more open than close braces, we are inside a block.
    open_braces > close_braces
}

pub async fn handle_hover(backend: &Backend, params: HoverParams) -> Result<Option<Hover>> {
    let start = Instant::now();
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let mut res: Option<Hover> = None;

    let Ok(path) = uri_to_path_buf(&uri) else {
        return Ok(None);
    };

    if let Some(resolved) = resolve_symbol_at(&backend.workspace, &uri, pos) {
        res = Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: resolved.target.hover_markdown(),
            }),
            range: Some(resolved.range),
        });
    } else if let Some(doc) = backend.document_map.get(&path) {
        if !is_inside_braces(&doc, pos) {
            if let Some(line) = doc.lines().nth(pos.line as usize) {
                let (start_char, end_char) = find_word_at_pos(&line.to_string(), pos.character);
                let word = &line.to_string()[start_char..end_char];

                if let Some(doc) = backend.workspace.keywords.get(word) {
                    let range = Range {
                        start: Position {
                            line: pos.line,
                            character: start_char as u32,
                        },
                        end: Position {
                            line: pos.line,
                            character: end_char as u32,
                        },
                    };
                    res = Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: doc.value().clone(),
                        }),
                        range: Some(range),
                    });
                }
            }
        }
    }

    let elapsed = start.elapsed();
    debug!(
        "hover in {}: {} L{}C{}",
        elapsed.log_str(),
        path.display(),
        pos.line + 1,
        pos.character + 1
    );
    Ok(res)
}
