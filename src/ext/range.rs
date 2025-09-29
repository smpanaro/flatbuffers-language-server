use tower_lsp::lsp_types::{Position, Range};

pub trait RangeExt {
    fn contains(&self, pos: Position) -> bool;
}

impl RangeExt for Range {
    fn contains(&self, pos: Position) -> bool {
        pos >= self.start && pos < self.end
    }
}

impl From<crate::ffi::Position> for Position {
    fn from(c_pos: crate::ffi::Position) -> Self {
        Position {
            line: c_pos.line,
            character: c_pos.col,
        }
    }
}

impl From<crate::ffi::Range> for Range {
    fn from(c_range: crate::ffi::Range) -> Self {
        Range {
            start: c_range.start.into(),
            end: c_range.end.into(),
        }
    }
}
