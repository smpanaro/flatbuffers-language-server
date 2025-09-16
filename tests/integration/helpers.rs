use tower_lsp::lsp_types::Position;

pub fn parse_fixture(fixture: &str) -> (String, Position) {
    let mut content = String::new();
    let mut position = Position::default();
    let mut found = false;

    for (line_num, line) in fixture.lines().enumerate() {
        if let Some(col) = line.find("$0") {
            if found {
                panic!("fixture must contain exactly one $0 cursor marker");
            }
            position.line = line_num as u32;
            position.character = col as u32;
            content.push_str(&line.replace("$0", ""));
            found = true;
        } else {
            content.push_str(line);
        }
        content.push('\n');
    }

    if !found {
        panic!("fixture must contain a $0 cursor marker");
    }

    // Remove the last newline
    content.pop();

    (content, position)
}
