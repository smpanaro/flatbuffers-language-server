use tower_lsp::lsp_types::{Position, Range};

pub trait RangeExt {
    fn contains(&self, pos: Position) -> bool;
}

impl RangeExt for Range {
    fn contains(&self, pos: Position) -> bool {
        (pos.line > self.start.line
            || (pos.line == self.start.line && pos.character >= self.start.character))
            && (pos.line < self.end.line
                || (pos.line == self.end.line && pos.character < self.end.character))
    }
}
