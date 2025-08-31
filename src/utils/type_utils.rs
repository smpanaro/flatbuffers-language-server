use tower_lsp::lsp_types::{Position, Range};

/// Extracts the base type name from a type string.
/// For vectors like "[Vec3]", returns "Vec3".
/// For arrays like "[uint:10]", returns "uint".
/// For regular types, returns the type as-is.
pub fn extract_base_type_name(type_name: &str) -> &str {
    if let Some(stripped) = type_name.strip_prefix('[') {
        if let Some(end_bracket) = stripped.rfind(']') {
            let inner = &stripped[..end_bracket];
            if let Some(colon_pos) = inner.find(':') {
                &inner[..colon_pos]
            } else {
                inner
            }
        } else {
            type_name
        }
    } else {
        type_name
    }
}

/// Calculates the range that covers only the inner type name for vectors/arrays.
/// For "[Vec3]", returns the range that covers only "Vec3".
/// For regular types, returns the original range.
pub fn calculate_inner_type_range(type_range: Range, type_name: &str) -> Range {
    if let Some(stripped) = type_name.strip_prefix('[') {
        if let Some(end_bracket) = stripped.rfind(']') {
            let inner = &stripped[..end_bracket];
            let base_type = if let Some(colon_pos) = inner.find(':') {
                &inner[..colon_pos]
            } else {
                inner
            };

            // Calculate the position of the inner type within the brackets
            let start_offset = 1; // Skip the opening bracket '['
            let base_type_len = base_type.chars().count() as u32;

            return Range::new(
                Position::new(
                    type_range.start.line,
                    type_range.start.character + start_offset,
                ),
                Position::new(
                    type_range.start.line,
                    type_range.start.character + start_offset + base_type_len,
                ),
            );
        }
    }

    // For non-vector/array types, return the original range
    type_range
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    #[test]
    fn test_extract_base_type_name() {
        assert_eq!(extract_base_type_name("Vec3"), "Vec3");
        assert_eq!(extract_base_type_name("[Vec3]"), "Vec3");
        assert_eq!(extract_base_type_name("[uint:10]"), "uint");
        assert_eq!(extract_base_type_name("string"), "string");
    }

    #[test]
    fn test_calculate_inner_type_range() {
        let range = Range::new(Position::new(0, 10), Position::new(0, 16)); // "[Vec3]"
        let inner_range = calculate_inner_type_range(range, "[Vec3]");
        assert_eq!(inner_range.start, Position::new(0, 11)); // Position of "V" in "Vec3"
        assert_eq!(inner_range.end, Position::new(0, 15)); // Position after "3" in "Vec3"

        let range = Range::new(Position::new(0, 5), Position::new(0, 13)); // "[uint:10]"
        let inner_range = calculate_inner_type_range(range, "[uint:10]");
        assert_eq!(inner_range.start, Position::new(0, 6)); // Position of "u" in "uint"
        assert_eq!(inner_range.end, Position::new(0, 10)); // Position after "t" in "uint"

        // Regular types should return the same range
        let range = Range::new(Position::new(0, 5), Position::new(0, 11)); // "string"
        let inner_range = calculate_inner_type_range(range, "string");
        assert_eq!(inner_range, range);
    }
}
