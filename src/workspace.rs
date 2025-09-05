use crate::symbol_table::{RootTypeInfo, Symbol, SymbolInfo, SymbolKind};
use dashmap::DashMap;
use tower_lsp::lsp_types::{Location, Range, Url};

#[derive(Debug)]
pub struct Workspace {
    /// All symbols defined in the workspace.
    pub symbols: DashMap<String, Symbol>,
    /// Symbols that are built-in to the FlatBuffers language.
    pub builtin_symbols: DashMap<String, Symbol>,
    /// Map from file URI to the list of symbol keys defined in that file.
    pub file_definitions: DashMap<Url, Vec<String>>,
    /// Map from file URI to the list of files it includes.
    pub file_includes: DashMap<Url, Vec<String>>,
    /// Map from file URI to the list of files that include it.
    pub file_included_by: DashMap<Url, Vec<Url>>,
    /// Map from file URI to the root type defined in that file.
    pub root_types: DashMap<Url, RootTypeInfo>,
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

impl Workspace {
    pub fn new() -> Self {
        let mut workspace = Self {
            symbols: DashMap::new(),
            builtin_symbols: DashMap::new(),
            file_definitions: DashMap::new(),
            file_includes: DashMap::new(),
            file_included_by: DashMap::new(),
            root_types: DashMap::new(),
        };
        populate_builtins(&mut workspace);
        workspace
    }

    pub fn update_symbols(
        &self,
        uri: &Url,
        st: crate::symbol_table::SymbolTable,
        included_files: Vec<String>,
        root_type_info: Option<crate::symbol_table::RootTypeInfo>,
    ) {
        if let Some((_, old_symbol_keys)) = self.file_definitions.remove(uri) {
            for key in old_symbol_keys {
                self.symbols.remove(&key);
            }
        }
        self.root_types.remove(uri);

        if let Some((_, old_included_files)) = self.file_includes.remove(uri) {
            for old_included_file in old_included_files {
                if let Ok(old_included_uri) = Url::from_file_path(&old_included_file) {
                    if let Some(mut included_by) = self.file_included_by.get_mut(&old_included_uri)
                    {
                        included_by.retain(|x| x != uri);
                    }
                }
            }
        }

        for included_file in &included_files {
            if let Ok(included_uri) = Url::from_file_path(included_file) {
                self.file_included_by
                    .entry(included_uri)
                    .or_default()
                    .push(uri.clone());
            }
        }

        let symbol_map = st.into_inner();
        let new_symbol_keys: Vec<String> = symbol_map.keys().cloned().collect();
        for (key, symbol) in symbol_map {
            self.symbols.insert(key, symbol);
        }
        self.file_definitions.insert(uri.clone(), new_symbol_keys);

        if let Some(rti) = root_type_info {
            self.root_types.insert(uri.clone(), rti);
        }

        self.file_includes.insert(uri.clone(), included_files);
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}
