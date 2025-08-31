use tower_lsp::lsp_types::{Position, Range};

pub trait RangeExt {
    fn contains(&self, pos: Position) -> bool;
}

impl RangeExt for Range {
    fn contains(&self, pos: Position) -> bool {
        pos >= self.start && pos < self.end
    }
}
