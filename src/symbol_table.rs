use std::collections::HashMap;
use tower_lsp::lsp_types::Location;

// A map from a fully qualified name to its symbol definition
pub type SymbolTable = HashMap<String, Symbol>;

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
    // ... other kinds like EnumVariant, Union, etc. will be added later
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
pub struct Field {
    pub type_name: String, // The name of the field's type, e.g., "string" or "Vec3"
}
