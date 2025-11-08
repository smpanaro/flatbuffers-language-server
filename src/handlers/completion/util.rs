use crate::analysis::WorkspaceSnapshot;
use crate::symbol_table::Symbol;
use crate::utils::as_pos_idx;
use ropey::Rope;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Position, Range, TextEdit};

pub fn generate_include_text_edit(
    snapshot: &WorkspaceSnapshot,
    path: &PathBuf,
    symbol: &Symbol,
) -> (Option<Vec<TextEdit>>, Option<String>) {
    if symbol.info.location.path != *path {
        let is_already_included = snapshot
            .dependencies
            .includes
            .get(path)
            .is_some_and(|includes| includes.iter().any(|p| p == &symbol.info.location.path));

        if !is_already_included {
            if let Some(relative_path) =
                pathdiff::diff_paths(&symbol.info.location.path, path.parent().unwrap())
            {
                if let Some(doc) = snapshot.documents.get(path) {
                    let edit = generate_include_edit(&doc, &relative_path.to_string_lossy());
                    let preview = edit.new_text.trim().strip_suffix(";").map(String::from);
                    return (Some(vec![edit]), preview);
                }
            }
        }
    }
    (None, None)
}

fn generate_include_edit(doc: &Rope, relative_path: &str) -> TextEdit {
    let last_include_line = doc
        .lines()
        .enumerate()
        .filter(|(_, line)| line.to_string().trim().starts_with("include "))
        .last()
        .map(|(i, _)| as_pos_idx(i));

    let include_insert_line = last_include_line.map_or(0, |line| line + 1);
    let include_insert_pos = Position::new(include_insert_line, 0);

    let mut new_text = format!("include \"{relative_path}\";\n");

    if let Some(line_after) = doc.lines().nth(include_insert_line as usize) {
        if !line_after.to_string().trim().is_empty() {
            new_text.push('\n');
        }
    }

    TextEdit {
        range: Range::new(include_insert_pos, include_insert_pos),
        new_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    #[test]
    fn test_generate_include_edit_no_includes_no_namespace() {
        let doc = Rope::from_str("table MyTable {}");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 0);
    }

    #[test]
    fn test_generate_include_edit_no_includes_with_namespace() {
        let doc = Rope::from_str("namespace MyNamespace;\n\ntable MyTable {}");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 0);
    }

    #[test]
    fn test_generate_include_edit_with_includes() {
        let doc = Rope::from_str("include \"b.fbs\";\n\nnamespace MyNamespace;");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n");
        assert_eq!(edit.range.start.line, 1);
    }

    #[test]
    fn test_generate_include_edit_with_includes_no_gap() {
        let doc = Rope::from_str("include \"b.fbs\";\nnamespace MyNamespace;");
        let edit = generate_include_edit(&doc, "a.fbs");
        assert_eq!(edit.new_text, "include \"a.fbs\";\n\n");
        assert_eq!(edit.range.start.line, 1);
    }
}
