use crate::symbol_table::{Location, Symbol, SymbolInfo, SymbolKind, SymbolTable};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp_server::lsp_types::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub doc: String,
    pub restricted_to_types: Option<Vec<String>>,
}

/// An index of known workspace symbols.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolIndex {
    /// Map from a fully-qualified name to its definition.
    pub global: HashMap<String, Symbol>,
    /// Map from a file path to the list of symbol keys defined in it.
    pub per_file: HashMap<PathBuf, Vec<String>>,
    /// Pre-populated, immutable map of built-in symbols.
    pub builtins: Arc<HashMap<String, Symbol>>,
    /// Pre-populated, immutable map of keywords.
    pub keywords: Arc<HashMap<String, String>>,
    /// Pre-populated, immutable map of built-in attributes.
    pub builtin_attributes: Arc<HashMap<String, Attribute>>,
    /// Mutable map of user-defined attributes.
    pub user_defined_attributes: HashMap<String, Attribute>,
    /// Map from a file path to the list of user-defined attributes declared in it.
    pub user_defined_attributes_per_file: HashMap<PathBuf, Vec<String>>,
}

impl SymbolIndex {
    #[must_use]
    pub fn new() -> Self {
        let mut builtins = HashMap::new();
        populate_builtins(&mut builtins);

        let mut keywords = HashMap::new();
        populate_keywords(&mut keywords);

        let mut builtin_attributes = HashMap::new();
        populate_builtin_attributes(&mut builtin_attributes);

        Self {
            global: HashMap::new(),
            per_file: HashMap::new(),
            builtins: Arc::new(builtins),
            keywords: Arc::new(keywords),
            builtin_attributes: Arc::new(builtin_attributes),
            user_defined_attributes: HashMap::new(),
            user_defined_attributes_per_file: HashMap::new(),
        }
    }

    pub fn update_symbols(&mut self, path: &Path, st: SymbolTable) {
        if let Some(old_symbol_keys) = self.per_file.remove(path) {
            for key in old_symbol_keys {
                self.global.remove(&key);
            }
        }

        let symbol_map = st.into_inner();
        let new_symbol_keys: Vec<String> = symbol_map
            .iter()
            .filter(|(_, v)| v.info.location.path == path)
            .map(|(k, _)| k)
            .cloned()
            .collect();

        for (key, symbol) in symbol_map {
            self.global.insert(key, symbol);
        }
        self.per_file.insert(path.to_path_buf(), new_symbol_keys);
    }

    pub fn update_attributes(&mut self, path: &Path, attributes: HashMap<String, String>) {
        // Clear old attributes for this path
        if let Some(old_attr_keys) = self.user_defined_attributes_per_file.remove(path) {
            for key in old_attr_keys {
                self.user_defined_attributes.remove(&key);
            }
        }

        // Add new attributes
        let new_attr_keys: Vec<String> = attributes.keys().cloned().collect();
        for (attr_name, doc) in attributes {
            self.user_defined_attributes.insert(
                attr_name.clone(),
                Attribute {
                    name: attr_name,
                    doc,
                    restricted_to_types: None,
                },
            );
        }
        if !new_attr_keys.is_empty() {
            self.user_defined_attributes_per_file
                .insert(path.to_path_buf(), new_attr_keys);
        }
    }

    pub fn remove(&mut self, path: &Path) {
        if let Some(old_symbol_keys) = self.per_file.remove(path) {
            for key in old_symbol_keys {
                self.global.remove(&key);
            }
        }
        if let Some(old_attr_keys) = self.user_defined_attributes_per_file.remove(path) {
            for key in old_attr_keys {
                self.user_defined_attributes.remove(&key);
            }
        }
    }

    #[must_use]
    pub fn namespaces(&self) -> HashSet<String> {
        self.global
            .values()
            .map(|s| &s.info.namespace)
            .filter(|ns| !ns.is_empty())
            .map(|ns| ns.join("."))
            .collect()
    }

    /// Returns a map from unqualified name to symbols that share that name.
    #[must_use]
    pub fn collisions(&self) -> HashMap<String, Vec<Symbol>> {
        let mut by_name: HashMap<String, Vec<Symbol>> = HashMap::new();
        for sym in self.global.values() {
            by_name
                .entry(sym.info.name.clone())
                .or_default()
                .push(sym.clone());
        }

        by_name.retain(|_, v| v.len() > 1);
        by_name
    }
}

// --- Built-in definitions ---

fn populate_builtins(symbols: &mut HashMap<String, Symbol>) {
    let scalar_types = [
        ("bool", "8-bit boolean"),
        ("byte", "8-bit signed integer"),
        ("ubyte", "8-bit unsigned integer"),
        ("short", "16-bit signed integer"),
        ("int16", "16-bit signed integer"),
        ("ushort", "16-bit unsigned integer"),
        ("uint16", "16-bit unsigned integer"),
        ("int", "32-bit signed integer"),
        ("int32", "32-bit signed integer"),
        ("uint", "32-bit unsigned integer"),
        ("uint32", "32-bit unsigned integer"),
        ("float", "32-bit single precision floating point"),
        ("float32", "32-bit single precision floating point"),
        ("long", "64-bit signed integer"),
        ("int64", "64-bit signed integer"),
        ("ulong", "64-bit unsigned integer"),
        ("uint64", "64-bit unsigned integer"),
        ("double", "64-bit double precision floating point"),
        ("float64", "64-bit double precision floating point"),
        (
            "string",
            "UTF-8 or 7-bit ASCII encoded string. For other text encodings or general binary data use vectors (`[byte]` or `[ubyte]`) instead.\n\nStored as zero-terminated string, prefixed by length.",
        ),
    ];

    for (type_name, doc) in scalar_types {
        let symbol = Symbol {
            info: SymbolInfo {
                name: type_name.to_string(),
                namespace: vec![],
                location: Location {
                    path: PathBuf::new(),
                    range: Range::default(),
                },
                documentation: Some(doc.to_string()),
                builtin: true,
            },
            kind: SymbolKind::Scalar,
        };
        symbols.insert(type_name.to_string(), symbol);
    }
}

#[allow(clippy::too_many_lines)]
fn populate_keywords(keywords: &mut HashMap<String, String>) {
    let keywords_data = [
        (
            "table",
            r"A type with fields.

The main way of grouping data in FlatBuffers. Fields can be added and removed while maintaining backwards compatibility allowing the type to evolve over time.

```flatbuffers
table Film {
    title:string;
    duration:int (deprecated);
}
```
",
        ),
        (
            "struct",
            r"A scalar type with fields.

All fields are required and must be scalar types, including other structs. Once defined structs cannot be modified. Structs use less memory than tables and are faster to access.

```flatbuffers
struct Vec3 {
    x:float;
    y:float;
    z:float;
}
```
",
        ),
        (
            "enum",
            r"A set of named constant values.

New values may be added, but old values cannot be removed or deprecated.

```flatbuffers
enum Size:byte {
  Small = 0,
  Medium,
  Large
}
```
",
        ),
        (
            "union",
            r"A set of possible table types.

Essentially an enum stored with a value that is one of its types.

```flatbuffers
table Photo { captured_at:uint64; }
table Video { duration:uint; }

union Medium {
    Photo,
    Video
}

table View {
    viewed_at:uint64;
    medium:Medium; // Which Photo or Video was viewed.
}
```
",
        ),
        (
            "namespace",
            r"Specify a namespace to use in generated code.

Support for this varies by language.

```flatbuffers
namespace Game.Core;

table Player {}
```

Generates the following C++:
```cpp
namespace Game {
  namespace Core {

    struct Player;
// ...
```
",
        ),
        (
            "root_type",
            r#"Declares the root table of a serialized FlatBuffer.

Must be a table. This is the "entry point" when reading serialized data.

```flatbuffers
table Discography {}

root_type Discography;
```

For example in Go:
```go
buf, err := os.ReadFile("discog.dat")
// handle err
discography := example.GetRootAsDiscography(buf, 0)
```
"#,
        ),
        (
            "include",
            r#"Include types from another schema file.

```flatbuffers
include "core.fbs";
```
"#,
        ),
        (
            "attribute",
            r#"A user-defined attribute that can be queried when parsing the schema.

```flatbuffers
attribute "internal_feature";

table Watch {
    brand:string;
    release_date:string (internal_feature);
}
```
"#,
        ),
        (
            "rpc_service",
            r"A set of functions that take a table as a request and return a table as a response.

Generated code support for this varies by language and RPC system.

```flatbuffers
rpc_service MonsterStorage {
    Store(Monster):StoreResponse;
    Retrieve(MonsterId):Monster;
}
```
",
        ),
    ];

    for (kw, doc) in &keywords_data {
        keywords.insert((*kw).to_string(), (*doc).to_string());
    }
}

fn populate_builtin_attributes(attributes: &mut HashMap<String, Attribute>) {
    const BUILTIN_ATTRIBUTES: &[(&str, &str, Option<&[&str]>)] = &[
        ("deprecated", "Omit generated code for this field.", None),
        ("required", "Require this field to be set. Generated code will enforce this.", None),
        ("key", "Use this field as a key for sorting vectors of its containing table.", None),
        (
            "hash",
            "Allow this field's JSON value to be a string, in which case its hash is stored in this field.",
            Some(&["uint32", "uint64", "uint", "ulong"]),
        ),
        ("force_align", "Force alignment to be higher than this struct or vector field's natural alignment.", None),
        (
            "nested_flatbuffer",
            "Mark this field as containing FlatBuffer data with the specified root type.",
            Some(&["[ubyte]", "[uint8]"]),
        ),
        (
            "flexbuffer",
            "Mark this field as containing FlexBuffer data.",
            Some(&["[ubyte]", "[uint8]"]),
        ),
        // ("bit_flags", "This enum's values are bit masks", None), // Only valid on enums. TODO: Support non-field attributes.
        // ("original_order", "Keep the original order of fields.", None), // Docs basically say don't use this.
    ];

    let attributes_data: Vec<Attribute> = BUILTIN_ATTRIBUTES
        .iter()
        .map(|(name, doc, restricted)| Attribute {
            name: (*name).into(),
            doc: (*doc).into(),
            restricted_to_types: restricted.map(|r| r.iter().map(|&s| s.into()).collect()),
        })
        .collect();

    for attr in attributes_data {
        attributes.insert(attr.name.clone(), attr);
    }
}

#[cfg(test)]
mod tests {
    use std::string::ToString;

    use super::*;
    use crate::symbol_table::{Location, Symbol, SymbolInfo, SymbolKind, Table};
    use tower_lsp_server::lsp_types::{Position, Range};

    fn make_symbol(name: &str, path: &Path) -> Symbol {
        let mut namespace = name.split('.').map(ToString::to_string).collect::<Vec<_>>();
        let unqualified_name = namespace.pop();
        Symbol {
            info: SymbolInfo {
                name: unqualified_name.unwrap(),
                namespace,
                location: Location {
                    path: path.to_path_buf(),
                    range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                },
                documentation: None,
                builtin: false,
            },
            kind: SymbolKind::Table(Table::default()),
        }
    }

    #[test]
    fn test_update_symbols() {
        let mut index = SymbolIndex::new();
        let path_a = PathBuf::from("a.fbs");
        let path_b = PathBuf::from("b.fbs");

        let mut st1 = SymbolTable::new(path_a.clone());
        st1.insert("A".to_string(), make_symbol("A", &path_a));

        index.update_symbols(&path_a, st1);
        assert_eq!(index.global.len(), 1);
        assert_eq!(index.per_file.get(&path_a).unwrap().len(), 1);

        let mut st2 = SymbolTable::new(path_a.clone());
        st2.insert("B".to_string(), make_symbol("B", &path_b));
        index.update_symbols(&path_a, st2);

        assert_eq!(index.global.len(), 1);
        assert!(index.global.contains_key("B"));
        assert!(!index.global.contains_key("A"));
        assert!(index.per_file.get(&path_a).unwrap().is_empty());
    }

    #[test]
    fn test_update_attributes() {
        let mut index = SymbolIndex::new();
        let path_a = PathBuf::from("a.fbs");
        let path_b = PathBuf::from("b.fbs");

        let mut attrs_a = HashMap::new();
        attrs_a.insert("attr1".to_string(), "doc1".to_string());
        attrs_a.insert("attr2".to_string(), "doc2".to_string());

        index.update_attributes(&path_a, attrs_a);
        assert_eq!(index.user_defined_attributes.len(), 2);
        assert_eq!(
            index
                .user_defined_attributes_per_file
                .get(&path_a)
                .unwrap()
                .len(),
            2
        );

        let mut attrs_b = HashMap::new();
        attrs_b.insert("attr3".to_string(), "doc3".to_string());
        index.update_attributes(&path_b, attrs_b);
        assert_eq!(index.user_defined_attributes.len(), 3);
        assert_eq!(
            index
                .user_defined_attributes_per_file
                .get(&path_b)
                .unwrap()
                .len(),
            1
        );

        // Test updating attributes for path_a
        let mut new_attrs_a = HashMap::new();
        new_attrs_a.insert("attr4".to_string(), "doc4".to_string());
        index.update_attributes(&path_a, new_attrs_a);
        assert_eq!(index.user_defined_attributes.len(), 2); // attr1, attr2 removed, attr4 added. attr3 remains.
        assert!(!index.user_defined_attributes.contains_key("attr1"));
        assert!(index.user_defined_attributes.contains_key("attr4"));
        assert_eq!(
            index
                .user_defined_attributes_per_file
                .get(&path_a)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn test_remove() {
        let mut index = SymbolIndex::new();
        let path_a = PathBuf::from("a.fbs");

        let mut st = SymbolTable::new(path_a.clone());
        st.insert("A".to_string(), make_symbol("A", &path_a));
        index.update_symbols(&path_a, st);

        let mut attrs = HashMap::new();
        attrs.insert("attr1".to_string(), "doc1".to_string());
        index.update_attributes(&path_a, attrs);

        assert_eq!(index.global.len(), 1);
        assert_eq!(index.per_file.len(), 1);
        assert_eq!(index.user_defined_attributes.len(), 1);
        assert_eq!(index.user_defined_attributes_per_file.len(), 1);

        index.remove(&path_a);
        assert!(index.global.is_empty());
        assert!(index.per_file.is_empty());
        assert!(index.user_defined_attributes.is_empty());
        assert!(index.user_defined_attributes_per_file.is_empty());
    }

    #[test]
    fn test_namespaces() {
        let mut index = SymbolIndex::new();
        let path_a = PathBuf::from("a.fbs");

        let mut st = SymbolTable::new(path_a.clone());
        for sym in [
            make_symbol("com.foo.bar.A", &path_a),
            make_symbol("com.foo.bar.B", &path_a),
            make_symbol("com.foo.C", &path_a),
            make_symbol("single.D", &path_a),
        ] {
            st.insert(sym.info.qualified_name(), sym);
        }

        index.update_symbols(&path_a, st);

        assert_eq!(
            HashSet::from_iter(vec![
                "com.foo.bar".to_string(),
                "com.foo".to_string(),
                "single".to_string()
            ]),
            index.namespaces()
        );
    }

    #[test]
    fn test_collisions() {
        let mut index = SymbolIndex::new();
        let path_a = PathBuf::from("a.fbs");

        let mut st = SymbolTable::new(path_a.clone());
        for sym in [
            make_symbol("com.foo.bar.Collides", &path_a),
            make_symbol("com.baz.qux.Collides", &path_a),
            make_symbol("com.foo.Unique", &path_a),
        ] {
            st.insert(sym.info.qualified_name(), sym);
        }

        index.update_symbols(&path_a, st);

        let collisions = index.collisions();
        assert_eq!(collisions.keys().len(), 1);
        assert_eq!(
            HashSet::from_iter(vec![
                "com.foo.bar.Collides".to_string(),
                "com.baz.qux.Collides".to_string()
            ]),
            collisions
                .get("Collides")
                .iter()
                .flat_map(|&syms| syms.iter())
                .map(|sym| sym.info.qualified_name())
                .collect::<HashSet<String>>()
        );
    }
}
