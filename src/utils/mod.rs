pub mod parsed_type;
pub mod paths;

/// Convert a usize to a u32 for use in `lsp_types::Position`.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn as_pos_idx(x: usize) -> u32 {
    x as u32
}
