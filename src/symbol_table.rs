use crate::utils::{parsed_type::ParsedType, paths::path_buf_to_uri};
use std::{collections::HashMap, path::PathBuf};
use tower_lsp_server::lsp_types::{self, CompletionItemKind, Position, Range};

use crate::ext::range::RangeExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub range: Range,
}

impl From<Location> for lsp_types::Location {
    fn from(val: Location) -> Self {
        lsp_types::Location {
            uri: path_buf_to_uri(&val.path).unwrap(),
            range: val.range,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RootTypeInfo {
    pub location: Location,
    pub type_name: String,
    pub parsed_type: ParsedType,
}

// A map from a fully qualified name to its symbol definition
#[derive(Debug)]
pub struct SymbolTable {
    pub path: PathBuf,
    table: HashMap<String, Symbol>,
}

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
    Scalar,
}

// Common information for all symbols
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub namespace: Vec<String>,
    pub location: Location,
    pub documentation: Option<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone)]
pub struct Table {
    pub fields: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub fields: Vec<Symbol>,
    pub size: usize,
    pub alignment: usize,
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
    pub parsed_type: ParsedType,
}

#[derive(Debug, Clone)]
pub struct Union {
    pub variants: Vec<UnionVariant>,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub type_name: String, // The name of the field's underlying type, e.g., "string" or "Vec3" (excludes vector/array tokens)
    pub type_display_name: String, // The fully-qualified name of the type, including vector and array tokens
    pub type_range: Range, // The full range covered by the type on the line. ie including brackets but not annotations
    pub parsed_type: ParsedType,
    pub deprecated: bool,
    pub has_id: bool,
    pub id: i32,
}

impl Symbol {
    pub fn type_name(&self) -> &str {
        match &self.kind {
            SymbolKind::Enum(_) => "enum",
            SymbolKind::Union(_) => "union",
            SymbolKind::Struct(_) => "struct",
            SymbolKind::Table(_) => "table",
            SymbolKind::Field(_) => "field",
            SymbolKind::Scalar => "scalar",
        }
    }

    pub fn find_symbol<'a>(&'a self, path: &PathBuf, pos: Position) -> Option<&'a Symbol> {
        if self.info.location.path != *path {
            return None;
        }

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
        let mut code_content = String::new();
        if !self.info.namespace.is_empty() {
            code_content.push_str(&format!("namespace {};\n\n", self.info.namespace.join(".")));
        }

        let definition = match &self.kind {
            SymbolKind::Table(t) => format!("table {} {{{}}}", self.info.name, t.fields_markdown()),
            SymbolKind::Struct(s) => {
                format!("struct {} {{{}}}", self.info.name, s.fields_markdown())
            }
            SymbolKind::Enum(e) => format!("enum {} {{{}}}", self.info.name, e.variants_markdown()),
            SymbolKind::Union(u) => {
                format!("union {} {{{}}}", self.info.name, u.variants_markdown())
            }
            SymbolKind::Scalar => format!("{} // scalar", self.info.name),
            SymbolKind::Field(f) => {
                format!("{}:{};", self.info.name, f.parsed_type.to_display_string())
            }
        };
        code_content.push_str(&definition);

        let mut markdown = format!("```flatbuffers\n{}\n```", code_content);

        if let Some(doc) = &self.info.documentation {
            if !doc.is_empty() {
                markdown.push_str("\n\n---\n\n");
                markdown.push_str(doc);
            }
        }

        if let SymbolKind::Struct(s) = &self.kind {
            markdown.push_str(
                format!(
                    "\n\n---\n\nSize: {} bytes\n\nAlignment: {} bytes",
                    s.size, s.alignment
                )
                .as_str(),
            );
        }

        markdown
    }
}

impl SymbolTable {
    /// Create a new token map.
    pub fn new(path: PathBuf) -> SymbolTable {
        SymbolTable {
            path: path,
            table: HashMap::with_capacity(2048),
        }
    }

    pub fn insert(&mut self, key: String, symbol: Symbol) {
        self.table.insert(key, symbol);
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.table.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&Symbol> {
        self.table.get(key)
    }

    pub fn values(&self) -> impl Iterator<Item = &Symbol> {
        self.table.values()
    }

    pub fn into_inner(self) -> HashMap<String, Symbol> {
        self.table
    }
}

fn fields_markdown(fields: &[Symbol]) -> String {
    if fields.is_empty() {
        return "".to_string();
    }
    format!(
        "\n{}\n",
        fields
            .iter()
            .filter_map(|field| {
                if let SymbolKind::Field(f) = &field.kind {
                    Some(format!(
                        "  {}:{};",
                        field.info.name,
                        f.parsed_type.to_display_string()
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<String>>()
            .join("\n")
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
            "\n{}\n",
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
                .join("\n")
        )
    }
}

impl Union {
    pub fn variants_markdown(&self) -> String {
        if self.variants.is_empty() {
            return "".to_string();
        }
        format!(
            "\n{}\n",
            self.variants
                .iter()
                .map(|v| format!("  {}", v.name))
                .collect::<Vec<String>>()
                .join("\n")
        )
    }
}

impl From<&SymbolKind> for CompletionItemKind {
    fn from(kind: &SymbolKind) -> Self {
        match kind {
            SymbolKind::Table(_) => CompletionItemKind::CLASS,
            SymbolKind::Struct(_) => CompletionItemKind::STRUCT,
            SymbolKind::Enum(_) => CompletionItemKind::ENUM,
            SymbolKind::Union(_) => CompletionItemKind::ENUM, // No specific kind for Union, Enum is close.
            SymbolKind::Field(_) => CompletionItemKind::FIELD,
            SymbolKind::Scalar => CompletionItemKind::KEYWORD,
        }
    }
}

impl SymbolInfo {
    pub fn qualified_name(&self) -> String {
        if self.namespace.is_empty() {
            self.name.clone()
        } else {
            format!("{}.{}", self.namespace.join("."), self.name)
        }
    }
}
