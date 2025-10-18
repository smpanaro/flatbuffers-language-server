use crate::parser::Parser;
use crate::symbol_table::{RootTypeInfo, Symbol, SymbolInfo, SymbolKind};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use tower_lsp::lsp_types::{Diagnostic, Location, Range, Url};

#[derive(Debug, Clone)]
pub struct Workspace {
    /// All symbols defined in the workspace.
    pub symbols: DashMap<String, Symbol>,
    /// Symbols that are built-in to the FlatBuffers language.
    pub builtin_symbols: DashMap<String, Symbol>,
    /// Keywords in the FlatBuffers language.
    pub keywords: DashMap<String, String>,
    /// Map from file URI to the list of symbol keys defined in that file.
    pub file_definitions: DashMap<Url, Vec<String>>,
    /// Map from file URI to the list of files it includes.
    pub file_includes: DashMap<Url, Vec<Url>>,
    /// Map from file URI to the list of files that include it.
    pub file_included_by: DashMap<Url, Vec<Url>>,
    /// Map from file URI to the root type defined in that file.
    pub root_types: DashMap<Url, RootTypeInfo>,
    pub published_diagnostics: DashMap<Url, Vec<Diagnostic>>,
    pub builtin_attributes: DashMap<String, Attribute>,
}

fn populate_builtins(workspace: &mut Workspace) {
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
                    uri: Url::parse("builtin:scalar").unwrap(),
                    range: Range::default(),
                },
                documentation: Some(doc.to_string()),
            },
            kind: SymbolKind::Scalar,
        };
        workspace
            .builtin_symbols
            .insert(type_name.to_string(), symbol);
    }
}

fn populate_keywords(workspace: &mut Workspace) {
    let keywords = [
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

    for (kw, doc) in keywords.iter() {
        workspace.keywords.insert(kw.to_string(), doc.to_string());
    }
}

impl Workspace {
    pub fn new() -> Self {
        let mut workspace = Self {
            symbols: DashMap::new(),
            builtin_symbols: DashMap::new(),
            keywords: DashMap::new(),
            file_definitions: DashMap::new(),
            file_includes: DashMap::new(),
            file_included_by: DashMap::new(),
            root_types: DashMap::new(),
            published_diagnostics: DashMap::new(),
            builtin_attributes: DashMap::new(),
        };
        populate_builtins(&mut workspace);
        populate_keywords(&mut workspace);
        populate_builtin_attributes(&mut workspace);
        workspace
    }

    pub fn update_symbols(
        &self,
        uri: &Url,
        st: crate::symbol_table::SymbolTable,
        included_files: Vec<Url>,
        root_type_info: Option<crate::symbol_table::RootTypeInfo>,
    ) {
        if let Some((_, old_symbol_keys)) = self.file_definitions.remove(uri) {
            for key in old_symbol_keys {
                self.symbols.remove(&key);
            }
        }
        self.root_types.remove(uri);

        self.update_includes(uri, included_files);

        let symbol_map = st.into_inner();
        let new_symbol_keys: Vec<String> = symbol_map.keys().cloned().collect();
        for (key, symbol) in symbol_map {
            self.symbols.insert(key, symbol);
        }
        self.file_definitions.insert(uri.clone(), new_symbol_keys);

        if let Some(rti) = root_type_info {
            self.root_types.insert(uri.clone(), rti);
        }
    }

    pub fn update_includes(&self, uri: &Url, included_uris: Vec<Url>) {
        if let Some((_, old_included_files)) = self.file_includes.remove(uri) {
            for old_included_uri in old_included_files {
                if let Some(mut included_by) = self.file_included_by.get_mut(&old_included_uri) {
                    included_by.retain(|x| x != uri);
                }
            }
        }

        for included_uri in &included_uris {
            self.file_included_by
                .entry(included_uri.clone())
                .or_default()
                .push(uri.clone());
        }

        self.file_includes.insert(uri.clone(), included_uris);
    }

    pub fn has_symbols_for(&self, uri: &Url) -> bool {
        self.file_definitions.contains_key(uri)
    }

    pub fn remove_file(&self, uri: &Url) -> Vec<Url> {
        if let Some((_, old_symbol_keys)) = self.file_definitions.remove(uri) {
            for key in old_symbol_keys {
                self.symbols.remove(&key);
            }
        }

        if let Some((_, included_files)) = self.file_includes.remove(uri) {
            for included_uri in included_files {
                if let Some(mut included_by) = self.file_included_by.get_mut(&included_uri) {
                    included_by.retain(|x| x != uri);
                }
            }
        }

        self.root_types.remove(uri);
        self.published_diagnostics.remove(uri);

        if let Some((_, included_by_files)) = self.file_included_by.remove(uri) {
            return included_by_files;
        }

        vec![]
    }

    pub async fn parse_and_update(
        &self,
        initial_uri: Url,
        document_map: &DashMap<String, ropey::Rope>,
        search_paths: &[Url],
        parsed_files: &mut HashSet<Url>,
    ) -> (HashMap<Url, Vec<Diagnostic>>, HashSet<Url>) {
        let mut files_to_parse = vec![initial_uri.clone()];
        let mut newly_parsed_files = HashSet::new();
        let mut all_diagnostics = std::collections::HashMap::new();

        while let Some(uri) = files_to_parse.pop() {
            if !parsed_files.insert(uri.clone()) {
                continue;
            }
            newly_parsed_files.insert(uri.clone());

            let content = if let Some(doc) = document_map.get(&uri.to_string()) {
                doc.value().to_string()
            } else {
                match tokio::fs::read_to_string(uri.to_file_path().unwrap()).await {
                    Ok(text) => {
                        document_map.insert(uri.to_string(), ropey::Rope::from_str(&text));
                        text
                    }
                    Err(e) => {
                        log::error!("failed to read file {}: {}", uri.path(), e);
                        continue;
                    }
                }
            };

            log::info!("parsing: {}", uri.clone().path());
            let (diagnostics_map, symbol_table, included_files, root_type_info) =
                crate::parser::FlatcFFIParser.parse(&uri, &content, search_paths);

            if let Some(st) = symbol_table {
                self.update_symbols(&uri, st, included_files.clone(), root_type_info);
            } else {
                // A parse error occurred, but we don't want to clear the old symbol table
                // as it may be useful to the user while they are editing.
                // We do want to make sure that we are tracking this file's existence,
                // in case it needs to be cleaned up later.
                if !self.file_definitions.contains_key(&uri) {
                    self.file_definitions.insert(uri.clone(), vec![]);
                }
                self.update_includes(&uri, included_files.clone());
            }

            for (file_uri, diagnostics) in diagnostics_map {
                all_diagnostics.insert(file_uri, diagnostics);
            }

            for included_uri in included_files {
                if !parsed_files.contains(&included_uri) {
                    files_to_parse.push(included_uri);
                }
            }
        }

        let mut files_to_update = HashSet::new();
        files_to_update.insert(initial_uri);
        files_to_update.extend(newly_parsed_files);

        (all_diagnostics, files_to_update)
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Attribute {
    pub name: String,
    pub doc: String,
    pub restricted_to_types: Option<Vec<String>>,
}

fn populate_builtin_attributes(workspace: &mut Workspace) {
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

    let attributes: Vec<Attribute> = BUILTIN_ATTRIBUTES
        .iter()
        .map(|(name, doc, restricted)| Attribute {
            name: (*name).into(),
            doc: (*doc).into(),
            restricted_to_types: restricted.map(|r| r.iter().map(|&s| s.into()).collect()),
        })
        .collect();

    for attr in attributes {
        workspace.builtin_attributes.insert(attr.name.clone(), attr);
    }
}
