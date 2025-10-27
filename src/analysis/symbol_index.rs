use crate::symbol_table::{Location, Symbol, SymbolInfo, SymbolKind, SymbolTable};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_lsp_server::lsp_types::Range;

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub doc: String,
    pub restricted_to_types: Option<Vec<String>>,
}

/// An index of known workspace symbols.
#[derive(Debug, Clone, Default)]
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
}

impl SymbolIndex {
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
        }
    }

    pub fn update(&mut self, path: &Path, st: SymbolTable) {
        if let Some(old_symbol_keys) = self.per_file.remove(path) {
            for key in old_symbol_keys {
                self.global.remove(&key);
            }
        }

        let symbol_map = st.into_inner();
        let new_symbol_keys: Vec<String> = symbol_map.keys().cloned().collect();
        for (key, symbol) in symbol_map {
            self.global.insert(key, symbol);
        }
        self.per_file.insert(path.to_path_buf(), new_symbol_keys);
    }

    pub fn remove(&mut self, path: &Path) {
        if let Some(old_symbol_keys) = self.per_file.remove(path) {
            for key in old_symbol_keys {
                self.global.remove(&key);
            }
        }
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

fn populate_keywords(keywords: &mut HashMap<String, String>) {
    let keywords_data = [
        (
            "table",
            r#"A type with fields.

The main way of grouping data in FlatBuffers. Fields can be added and removed while maintaining backwards compatibility allowing the type to evolve over time.

```flatbuffers
table Film {
    title:string;
    duration:int (deprecated);
}
```
"#,
        ),
        (
            "struct",
            r#"A scalar type with fields.

All fields are required and must be scalar types, including other structs. Once defined structs cannot be modified. Structs use less memory than tables and are faster to access.

```flatbuffers
struct Vec3 {
    x:float;
    y:float;
    z:float;
}
```
"#,
        ),
        (
            "enum",
            r#"A set of named constant values.

New values may be added, but old values cannot be removed or deprecated.

```flatbuffers
enum Size:byte {
  Small = 0,
  Medium,
  Large
}
```
"#,
        ),
        (
            "union",
            r#"A set of possible table types.

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
"#,
        ),
        (
            "namespace",
            r#"Specify a namespace to use in generated code.

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
"#,
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
"#,
        ),
    ];

    for (kw, doc) in keywords_data.iter() {
        keywords.insert(kw.to_string(), doc.to_string());
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
