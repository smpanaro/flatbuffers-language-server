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
pub struct EnumVariant {
    pub name: String,
    pub value: i64,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub name: String,
    pub location: Location,
}

#[derive(Debug, Clone)]
pub struct Union {
    pub variants: Vec<UnionVariant>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub type_name: String, // The name of the field's type, e.g., "string" or "Vec3"
    pub type_range: Range,
}

impl Symbol {
    pub fn find_symbol<'a>(&'a self, pos: Position) -> Option<&'a Symbol> {
        if self.info.location.range.contains(pos) {
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
                }
            }
            SymbolKind::Struct(s) => {
                for field in &s.fields {
                    if let SymbolKind::Field(f) = &field.kind {
                        if f.type_range.contains(pos) {
                            return Some(field);
                        }
                    }
                }
            }
            SymbolKind::Union(u) => {
                for variant in &u.variants {
                    if variant.location.range.contains(pos) {
                        return Some(self);
                    }
                }
            }
            _ => {}
        }

        None
    }

    pub fn hover_markdown(&self) -> String {
        let mut markdown = format!(
            "```flatbuffers
{}
```",
            match &self.kind {
                SymbolKind::Table(t) =>
                    format!("table {} {{{}}}", self.info.name, t.fields_markdown()),
                SymbolKind::Struct(s) =>
                    format!("struct {} {{{}}}", self.info.name, s.fields_markdown()),
                SymbolKind::Enum(e) =>
                    format!("enum {} {{{}}}", self.info.name, e.variants_markdown()),
                SymbolKind::Union(u) =>
                    format!("union {} {{{}}}", self.info.name, u.variants_markdown()),
                SymbolKind::Field(f) => format!("{}: {}", self.info.name, f.type_name),
            }
        );

        if let Some(doc) = &self.info.documentation {
            if !doc.is_empty() {
                markdown.push_str(
                    "

---

",
                );
                markdown.push_str(doc);
            }
        }

        markdown
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
        self.0.values().find_map(|symbol| symbol.find_symbol(pos))
    }
}

fn fields_markdown(fields: &[Symbol]) -> String {
    if fields.is_empty() {
        return "".to_string();
    }
    format!(
        "
{}
",
        fields
            .iter()
            .filter_map(|field| {
                if let SymbolKind::Field(f) = &field.kind {
                    Some(format!("  {}: {}", field.info.name, &f.type_name))
                } else {
                    None
                }
            })
            .collect::<Vec<String>>()
            .join(
                "
"
            )
    )
}

impl Table {
    pub fn fields_markdown(&self) -> String {
        fields_markdown(&self.fields)
    }
}

impl Struct {
    pub fn fields_markdown(&self) -> String {
        fields_markdown(&self.fields)
    }
}

impl Enum {
    pub fn variants_markdown(&self) -> String {
        if self.variants.is_empty() {
            return "".to_string();
        }
        format!(
            "
{}
",
            self.variants
                .iter()
                .map(|v| {
                    let mut s = format!("  {} = {}", v.name, v.value);
                    if let Some(doc) = &v.documentation {
                        if !doc.is_empty() {
                            s.push_str(&format!("\n  /// {}", doc.replace('\n', "\n  /// ")));
                        }
                    }
                    s
                })
                .collect::<Vec<String>>()
                .join(
                    "
"
                )
        )
    }
}

impl Union {
    pub fn variants_markdown(&self) -> String {
        if self.variants.is_empty() {
            return "".to_string();
        }
        format!(
            "
{}
",
            self.variants
                .iter()
                .map(|v| format!("  {}", v.name))
                .collect::<Vec<String>>()
                .join(
                    "
"
                )
        )
    }
}
