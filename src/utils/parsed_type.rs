use tower_lsp::lsp_types::{Position, Range};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypePart {
    pub text: String,
    pub range: Range,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedType {
    pub is_vector: bool,
    pub namespace: Vec<TypePart>,
    pub type_name: TypePart,
    pub array_size: Option<TypePart>,
}

pub fn parse_type(text: &str, range: Range) -> ParsedType {
    let mut parser = TypeParser::new(text, range.start);
    parser.parse()
}

struct TypeParser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    current_pos: Position,
}

impl<'a> TypeParser<'a> {
    fn new(text: &'a str, start_pos: Position) -> Self {
        Self {
            chars: text.chars().peekable(),
            current_pos: start_pos,
        }
    }

    fn advance(&mut self) -> Option<char> {
        let char = self.chars.next()?;
        if char == '\n' {
            self.current_pos.line += 1;
            self.current_pos.character = 0;
        } else {
            self.current_pos.character += 1;
        }
        Some(char)
    }

    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.chars.peek() == Some(&expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn parse_part(&mut self) -> TypePart {
        let start = self.current_pos;
        let mut text = String::new();
        while let Some(&c) = self.chars.peek() {
            match c {
                '.' | ':' | '[' | ']' | ' ' | '\t' | '\\' | '\n' => break,
                _ => {
                    text.push(c);
                    self.advance();
                }
            }
        }
        let end = self.current_pos;
        TypePart {
            text,
            range: Range::new(start, end),
        }
    }

    fn parse_fqn(&mut self) -> (Vec<TypePart>, TypePart) {
        let mut parts = Vec::new();
        loop {
            let part = self.parse_part();
            let is_empty = part.text.is_empty();
            if !is_empty {
                parts.push(part);
            }

            self.skip_whitespace();
            if self.consume_char('.') {
                self.skip_whitespace();
                if is_empty {
                    continue;
                }
            } else {
                break;
            }
        }
        let type_name = parts.pop().expect("Type should have at least a name");
        (parts, type_name)
    }

    fn parse(&mut self) -> ParsedType {
        self.skip_whitespace();
        let is_vector = self.consume_char('[');
        self.skip_whitespace();

        let (namespace, type_name) = self.parse_fqn();

        let mut array_size = None;
        self.skip_whitespace();
        if self.consume_char(':') {
            self.skip_whitespace();
            array_size = Some(self.parse_part());
        }

        self.skip_whitespace();
        if is_vector {
            self.consume_char(']');
        }
        self.skip_whitespace();

        ParsedType {
            is_vector,
            namespace,
            type_name,
            array_size,
        }
    }
}

impl ParsedType {
    pub fn qualified_name(&self) -> String {
        let mut parts = self
            .namespace
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>();
        parts.push(&self.type_name.text);
        parts.join(".")
    }
    pub fn to_display_string(&self) -> String {
        let mut s = String::new();
        if self.is_vector {
            s.push('[');
        }

        let mut parts = self
            .namespace
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>();
        parts.push(&self.type_name.text);
        s.push_str(&parts.join("."));

        if let Some(size) = &self.array_size {
            s.push(':');
            s.push_str(&size.text);
        }

        if self.is_vector {
            s.push(']');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    #[test]
    fn test_simple_type() {
        let text = "MyType";
        let range = Range::new(Position::new(1, 10), Position::new(1, 16));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: false,
                namespace: vec![],
                type_name: TypePart {
                    text: "MyType".to_string(),
                    range: Range::new(Position::new(1, 10), Position::new(1, 16)),
                },
                array_size: None,
            }
        );
    }

    #[test]
    fn test_namespaced_type() {
        let text = "My.Name.Space.Type";
        let range = Range::new(Position::new(2, 5), Position::new(2, 23));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: false,
                namespace: vec![
                    TypePart {
                        text: "My".to_string(),
                        range: Range::new(Position::new(2, 5), Position::new(2, 7))
                    },
                    TypePart {
                        text: "Name".to_string(),
                        range: Range::new(Position::new(2, 8), Position::new(2, 12))
                    },
                    TypePart {
                        text: "Space".to_string(),
                        range: Range::new(Position::new(2, 13), Position::new(2, 18))
                    },
                ],
                type_name: TypePart {
                    text: "Type".to_string(),
                    range: Range::new(Position::new(2, 19), Position::new(2, 23)),
                },
                array_size: None,
            }
        );
    }

    #[test]
    fn test_vector_type() {
        let text = "[MyType]";
        let range = Range::new(Position::new(3, 12), Position::new(3, 20));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: true,
                namespace: vec![],
                type_name: TypePart {
                    text: "MyType".to_string(),
                    range: Range::new(Position::new(3, 13), Position::new(3, 19)),
                },
                array_size: None,
            }
        );
    }

    #[test]
    fn test_vector_with_array_size() {
        let text = "[MyType:123]";
        let range = Range::new(Position::new(4, 4), Position::new(4, 16));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: true,
                namespace: vec![],
                type_name: TypePart {
                    text: "MyType".to_string(),
                    range: Range::new(Position::new(4, 5), Position::new(4, 11)),
                },
                array_size: Some(TypePart {
                    text: "123".to_string(),
                    range: Range::new(Position::new(4, 12), Position::new(4, 15)),
                }),
            }
        );
    }

    #[test]
    fn test_namespaced_vector_with_array_size() {
        let text = "[My.Name.Space.Type:42]";
        let range = Range::new(Position::new(5, 2), Position::new(5, 25));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: true,
                namespace: vec![
                    TypePart {
                        text: "My".to_string(),
                        range: Range::new(Position::new(5, 3), Position::new(5, 5))
                    },
                    TypePart {
                        text: "Name".to_string(),
                        range: Range::new(Position::new(5, 6), Position::new(5, 10))
                    },
                    TypePart {
                        text: "Space".to_string(),
                        range: Range::new(Position::new(5, 11), Position::new(5, 16))
                    },
                ],
                type_name: TypePart {
                    text: "Type".to_string(),
                    range: Range::new(Position::new(5, 17), Position::new(5, 21)),
                },
                array_size: Some(TypePart {
                    text: "42".to_string(),
                    range: Range::new(Position::new(5, 22), Position::new(5, 24)),
                }),
            }
        );
    }

    #[test]
    fn test_with_whitespace() {
        // Note: Inputs should not have leading or trailing whitespace, but handling it is ok.
        let text = "  [  My.Type: 123 ]  ";
        let range = Range::new(Position::new(6, 1), Position::new(6, 22));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: true,
                namespace: vec![TypePart {
                    text: "My".to_string(),
                    range: Range::new(Position::new(6, 6), Position::new(6, 8))
                },],
                type_name: TypePart {
                    text: "Type".to_string(),
                    range: Range::new(Position::new(6, 9), Position::new(6, 13)),
                },
                array_size: Some(TypePart {
                    text: "123".to_string(),
                    range: Range::new(Position::new(6, 15), Position::new(6, 18)),
                }),
            }
        );
    }

    #[test]
    fn test_multiline_vector() {
        let text = "[
  MyType
]";
        let range = Range::new(Position::new(10, 4), Position::new(12, 5));
        let parsed = parse_type(text, range);
        assert_eq!(
            parsed,
            ParsedType {
                is_vector: true,
                namespace: vec![],
                type_name: TypePart {
                    text: "MyType".to_string(),
                    range: Range::new(Position::new(11, 2), Position::new(11, 8)),
                },
                array_size: None,
            }
        );
    }
}
