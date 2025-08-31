use std::collections::HashMap;
use tower_lsp::lsp_types::{Location, Position, Range};

use crate::ext::range::RangeExt;

// A map from a fully qualified name to its symbol definition
#[derive(Debug)]
pub struct SymbolTable(HashMap<String, Symbol>);

// Represents a single symbol in the source code
#[derive(Debug, Clone)]
pub struct Symbol {
    pub info: SymbolInfo,
    pub kind: SymbolKind,
}

// The kind of a symbol
#[derive(Debug, Clone)]
pub enum SymbolKind {
    Table(Table),
    Struct(Struct),
    Enum(Enum),
    Field(Field),
    Union(Union),
    // ... other kinds like EnumVariant, etc. will be added later
}

// Common information for all symbols
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub location: Location,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub fields: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub fields: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    // For now, we just care that it exists.
    // We will add variants later.
}

#[derive(Debug, Clone)]
pub struct Union {
    // TODO: Add type/unionfield/variant.
}

#[derive(Debug, Clone)]
pub struct Field {
    pub type_name: String, // The name of the field's type, e.g., "string" or "Vec3"
    pub type_range: Range,
}

impl Symbol {
    pub fn find_symbol<'a>(&'a self, pos: Position) -> Option<&'a Symbol> {
        let range = self.info.location.range;

        if range.contains(pos) {
            return Some(self);
        }

        match &self.kind {
            SymbolKind::Table(t) => {
                for field in &t.fields {
                    if let SymbolKind::Field(f) = &field.kind {
                        if f.type_range.contains(pos) {
                            return Some(field);
                        }
                    }
                    // No need to recurse, hovering the field name is not useful.
                }
            }
            SymbolKind::Struct(s) => {
                for field in &s.fields {
                    if let SymbolKind::Field(f) = &field.kind {
                        if f.type_range.contains(pos) {
                            return Some(field);
                        }
                    }
                    // No need to recurse, hovering the field name is not useful.
                }
            }
            SymbolKind::Enum(_) => {
                // later: variants
            }
            SymbolKind::Union(_) => {
                // later: fields
            }
            SymbolKind::Field(_) => {
                // leaf
            }
        }

        None
    }

    pub fn hover_markdown(&self) -> String {
        format!(
            "```flatbuffers\n{}\n```{}",
            match &self.kind {
                SymbolKind::Table(t) =>
                    format!("table {} {{{}}}", self.info.name, t.fields_markdown()),
                SymbolKind::Struct(s) =>
                    format!("struct {} {{{}}}", self.info.name, s.fields_markdown()),
                SymbolKind::Enum(_) => format!("enum {}", self.info.name),
                SymbolKind::Union(_) => format!("union {}", self.info.name),
                SymbolKind::Field(_) => "".to_string(),
            },
            self.info
                .documentation
                .as_deref()
                .map(|d| format!("\n---\n{}", d))
                .unwrap_or("".to_string()),
        )
    }
}

impl SymbolTable {
    /// Create a new token map.
    pub fn new() -> SymbolTable {
        SymbolTable(HashMap::with_capacity(2048))
    }

    pub fn insert(&mut self, symbol: Symbol) {
        self.0.insert(symbol.info.name.clone(), symbol);
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn values(&self) -> impl Iterator<Item = &Symbol> {
        self.0.values()
    }

    pub fn get(&self, key: &str) -> Option<&Symbol> {
        self.0.get(key)
    }

    pub fn find_in_table<'a>(&'a self, pos: Position) -> Option<&'a Symbol> {
        for symbol in self.0.values() {
            if let Some(found) = symbol.find_symbol(pos) {
                return Some(found);
            }
        }
        None
    }
}

pub trait FieldsMarkdown {
    fn fields_markdown(&self) -> String;
}

impl FieldsMarkdown for [Symbol] {
    fn fields_markdown(&self) -> String {
        if self.iter().count() == 0 {
            return "".to_string();
        }
        format!(
            "\n{}\n",
            self.iter()
                .map(|field| match &field.kind {
                    SymbolKind::Field(f) => format!("  {}: {}", field.info.name, &f.type_name),
                    _ => "".to_string(),
                })
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}

impl Table {
    pub fn fields_markdown(&self) -> String {
        self.fields.fields_markdown()
    }
}

impl Struct {
    pub fn fields_markdown(&self) -> String {
        self.fields.fields_markdown()
    }
}
