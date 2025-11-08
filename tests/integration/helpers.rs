use flatbuffers_language_server::utils::as_pos_idx;
use tower_lsp_server::lsp_types::Position;

pub fn parse_fixture(fixture: &str) -> (String, Position) {
    let mut content = String::new();
    let mut position = Position::default();
    let mut found = false;

    for (line_num, line) in fixture.lines().enumerate() {
        if let Some(col) = line.find("$0") {
            assert!(!found, "fixture must contain exactly one $0 cursor marker");
            position.line = as_pos_idx(line_num);
            position.character = as_pos_idx(col);
            content.push_str(&line.replace("$0", ""));
            found = true;
        } else {
            content.push_str(line);
        }
        content.push('\n');
    }

    assert!(found, "fixture must contain a $0 cursor marker");

    // Remove the last newline
    content.pop();

    (content, position)
}
