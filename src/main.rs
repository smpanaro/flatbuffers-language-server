use crate::symbol_table::Symbol;
use dashmap::DashMap;
use log::{debug, error, info};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{OneOf, *};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::ext::range::RangeExt;
use crate::lsp_logger::LspLogger;
use crate::parser::{FlatcFFIParser, Parser};
use crate::workspace::Workspace;
use tokio::fs;

mod ext;
mod ffi;
mod lsp_logger;
mod parser;
mod symbol_table;
mod utils;
mod workspace;

static FIELD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(\w+)\s*:").unwrap());

#[derive(Debug)]
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
    workspace: Workspace,
    parser: FlatcFFIParser,
}

/// Represents what symbol was found at a given location, and what it resolves to.
#[derive(Debug, Clone)]
struct ResolvedSymbol {
    /// The symbol that is the ultimate "target" of the hover or go-to-definition.
    target: Symbol,
    /// The specific range of the text that was hovered or clicked (e.g., the range of a field's type).
    range: Range,
    /// The name of the symbol to use when finding references.
    ref_name: String,
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
            info: symbol_table::SymbolInfo {
                name: type_name.to_string(),
                location: Location {
                    uri: Url::parse("builtin:scalar").unwrap(),
                    range: Range::default(),
                },
                documentation: Some(doc.to_string()),
            },
            kind: symbol_table::SymbolKind::Scalar,
        };
        workspace
            .builtin_symbols
            .insert(type_name.to_string(), symbol);
    }
}

impl Backend {
    async fn parse_and_discover(&self, initial_uri: Url, initial_content: Option<String>) {
        let mut files_to_parse = vec![(initial_uri, initial_content)];
        let mut parsed_files = HashSet::new();

        while let Some((uri, content_opt)) = files_to_parse.pop() {
            if !parsed_files.insert(uri.clone()) {
                continue;
            }

            let content = if let Some(c) = content_opt {
                c
            } else if let Some(doc) = self.document_map.get(&uri.to_string()) {
                doc.value().clone()
            } else {
                match fs::read_to_string(uri.to_file_path().unwrap()).await {
                    Ok(text) => text,
                    Err(e) => {
                        error!("Failed to read file {}: {}", uri, e);
                        continue;
                    }
                }
            };

            self.document_map.insert(uri.to_string(), content.clone());

            let start_time = Instant::now();
            let (diagnostics, symbol_table, included_files, root_type_info) =
                self.parser.parse(&uri, &content);
            let elapsed_time = start_time.elapsed();
            error!("Parsed in {}ms: {}", elapsed_time.as_millis(), uri);

            // Only update workspace state if the parse was successful.
            // Otherwise, we would be clearing symbols for a file that has a transient syntax error.
            if let Some(st) = symbol_table {
                // Clear old symbols for this file
                if let Some((_, old_symbol_keys)) = self.workspace.file_definitions.remove(&uri) {
                    for key in old_symbol_keys {
                        self.workspace.symbols.remove(&key);
                    }
                }
                self.workspace.root_types.remove(&uri);

                let symbol_map = st.into_inner();
                let new_symbol_keys: Vec<String> = symbol_map.keys().cloned().collect();
                for (key, symbol) in symbol_map {
                    self.workspace.symbols.insert(key, symbol);
                }
                self.workspace
                    .file_definitions
                    .insert(uri.clone(), new_symbol_keys);

                if let Some(rti) = root_type_info {
                    self.workspace.root_types.insert(uri.clone(), rti);
                }

                self.workspace
                    .file_includes
                    .insert(uri.clone(), included_files.clone());
            }

            self.client
                .publish_diagnostics(uri.clone(), diagnostics, None)
                .await;

            for included_path_str in included_files {
                match Url::from_file_path(&included_path_str) {
                    Ok(included_uri) => {
                        if !parsed_files.contains(&included_uri) {
                            files_to_parse.push((included_uri, None));
                        }
                    }
                    Err(_) => {
                        error!("Invalid include path: {}", included_path_str);
                    }
                }
            }
        }
    }

    fn resolve_symbol_at(&self, uri: &Url, position: Position) -> Option<ResolvedSymbol> {
        // Check if the cursor is on a root_type declaration
        if let Some(root_type_info) = self.workspace.root_types.get(uri) {
            if root_type_info.location.range.contains(position) {
                if let Some(target_symbol) = self.workspace.symbols.get(&root_type_info.type_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.value().clone(),
                        range: root_type_info.location.range,
                        ref_name: root_type_info.type_name.clone(),
                    });
                }
            }
        }

        let symbol_at_cursor = self
            .workspace
            .symbols
            .iter()
            .find_map(|entry| entry.value().find_symbol(uri, position).cloned())?;

        if let symbol_table::SymbolKind::Union(u) = &symbol_at_cursor.kind {
            for variant in &u.variants {
                if variant.location.range.contains(position) {
                    let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                    if let Some(target_symbol) = self.workspace.symbols.get(base_name) {
                        return Some(ResolvedSymbol {
                            target: target_symbol.value().clone(),
                            range: variant.location.range,
                            ref_name: base_name.to_string(),
                        });
                    // Technically this isn't supported currently.
                    } else if let Some(target_symbol) =
                        self.workspace.builtin_symbols.get(base_name)
                    {
                        return Some(ResolvedSymbol {
                            target: target_symbol.clone(),
                            range: variant.location.range,
                            ref_name: base_name.to_string(),
                        });
                    }
                    return None;
                }
            }
        }

        if let symbol_table::SymbolKind::Field(f) = &symbol_at_cursor.kind {
            let inner_type_range =
                utils::type_utils::calculate_inner_type_range(f.type_range, &f.type_name);
            if inner_type_range.contains(position) {
                let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
                if let Some(target_symbol) = self.workspace.symbols.get(base_type_name) {
                    return Some(ResolvedSymbol {
                        target: target_symbol.value().clone(),
                        range: inner_type_range,
                        ref_name: base_type_name.to_string(),
                    });
                } else if let Some(target_symbol) =
                    self.workspace.builtin_symbols.get(base_type_name)
                {
                    return Some(ResolvedSymbol {
                        target: target_symbol.clone(),
                        range: inner_type_range,
                        ref_name: base_type_name.to_string(),
                    });
                }
                return None;
            }
        }

        // Default case: the symbol at cursor is the target.
        let range = symbol_at_cursor.info.location.range;
        let ref_name = symbol_at_cursor.info.name.clone();
        Some(ResolvedSymbol {
            target: symbol_at_cursor,
            range,
            ref_name,
        })
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        info!("Initializing server...");
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "flatbuffers-language-server".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![":".to_string(), " ".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                        ..CodeActionOptions::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Server initialized!");
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down server...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        debug!("Opened: {}", params.text_document.uri);
        self.parse_and_discover(params.text_document.uri, Some(params.text_document.text))
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        debug!("Changed: {}", params.text_document.uri);
        self.parse_and_discover(
            params.text_document.uri,
            Some(params.content_changes.remove(0).text),
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("closed: {}", params.text_document.uri);
        self.document_map
            .remove(&params.text_document.uri.to_string());

        // Remove symbols defined in the closed file
        if let Some((_, old_symbol_keys)) = self
            .workspace
            .file_definitions
            .remove(&params.text_document.uri)
        {
            for key in old_symbol_keys {
                self.workspace.symbols.remove(&key);
            }
        }

        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let start = Instant::now();
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let res = self.resolve_symbol_at(&uri, pos).map(|resolved| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: resolved.target.hover_markdown(),
            }),
            range: Some(resolved.range),
        });

        let elapsed = start.elapsed();
        info!(
            "hover in {}ms: {} L{}C{}",
            elapsed.as_millis(),
            &uri.path(),
            pos.line + 1,
            pos.character + 1
        );
        Ok(res)
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(resolved) = self.resolve_symbol_at(&uri, position) else {
            return Ok(None);
        };

        if resolved.target.info.location.uri.scheme() == "builtin" {
            return Ok(None);
        }

        Ok(Some(GotoDefinitionResponse::Scalar(
            resolved.target.info.location.clone(),
        )))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(resolved) = self.resolve_symbol_at(&uri, position) else {
            return Ok(None);
        };

        if resolved.target.info.location.uri.scheme() == "builtin" {
            return Ok(None);
        }

        let target_name = resolved.ref_name;
        let mut references = Vec::new();

        // Find all references to this symbol across all files
        for entry in self.workspace.symbols.iter() {
            let symbol = entry.value();
            let file_uri = &symbol.info.location.uri;

            // Check nested fields in tables and structs
            if let symbol_table::SymbolKind::Table(t) = &symbol.kind {
                for field in &t.fields {
                    if let symbol_table::SymbolKind::Field(f) = &field.kind {
                        let base_type_name =
                            utils::type_utils::extract_base_type_name(&f.type_name);
                        if base_type_name == target_name {
                            let inner_type_range = utils::type_utils::calculate_inner_type_range(
                                f.type_range,
                                &f.type_name,
                            );
                            references.push(Location {
                                uri: file_uri.clone(),
                                range: inner_type_range,
                            });
                        }
                    }
                }
            }

            if let symbol_table::SymbolKind::Struct(s) = &symbol.kind {
                for field in &s.fields {
                    if let symbol_table::SymbolKind::Field(f) = &field.kind {
                        let base_type_name =
                            utils::type_utils::extract_base_type_name(&f.type_name);
                        if base_type_name == target_name {
                            let inner_type_range = utils::type_utils::calculate_inner_type_range(
                                f.type_range,
                                &f.type_name,
                            );
                            references.push(Location {
                                uri: file_uri.clone(),
                                range: inner_type_range,
                            });
                        }
                    }
                }
            }

            if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
                for variant in &u.variants {
                    let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                    if base_name == target_name {
                        references.push(Location {
                            uri: file_uri.clone(),
                            range: variant.location.range,
                        });
                    }
                }
            }
        }

        // Include the definition itself if requested
        if params.context.include_declaration {
            if let Some(def_symbol) = self.workspace.symbols.get(&target_name) {
                if def_symbol.info.location.uri.scheme() != "builtin" {
                    references.push(def_symbol.info.location.clone());
                }
            }
        }

        let elapsed = start.elapsed();
        info!(
            "references in {}ms: {} L{}C{} -> {} refs",
            elapsed.as_millis(),
            &uri.path(),
            position.line + 1,
            position.character + 1,
            references.len()
        );

        Ok(if references.is_empty() {
            None
        } else {
            Some(references)
        })
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let start = Instant::now();
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(doc) = self.document_map.get(uri.as_str()) else {
            return Ok(None);
        };
        let Some(line) = doc.lines().nth(position.line as usize) else {
            return Ok(None);
        };

        let curr_char = line
            .chars()
            .nth(position.character.saturating_sub(1) as usize);
        let prev_char = line
            .chars()
            .nth(position.character.saturating_sub(2) as usize);
        if curr_char == Some(' ') && prev_char != Some(':') {
            return Ok(None);
        }

        let Some(captures) = FIELD_RE.captures(line) else {
            return Ok(None);
        };
        let field_name = captures.get(1).map_or("", |m| m.as_str());

        let mut items = Vec::new();

        // User-defined symbols
        for entry in self.workspace.symbols.iter() {
            let symbol = entry.value();
            let kind = (&symbol.kind).into();

            if kind != CompletionItemKind::FIELD {
                let label = symbol.info.name.clone();
                let sort_text = if field_name.to_lowercase().contains(&label.to_lowercase()) {
                    format!("0_{}", label)
                } else {
                    format!("1_{}", label)
                };

                items.push(CompletionItem {
                    label,
                    sort_text: Some(sort_text),
                    kind: Some(kind),
                    detail: Some(symbol.type_name().to_string()),
                    documentation: symbol.info.documentation.as_ref().map(|doc| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: doc.clone(),
                        })
                    }),
                    ..Default::default()
                });
            }
        }

        // Built-in symbols
        for (name, symbol) in self.workspace.builtin_symbols.iter() {
            let sort_text = if field_name.to_lowercase().contains(name) {
                format!("0_{}", name)
            } else {
                format!("1_{}", name)
            };
            items.push(CompletionItem {
                label: name.clone(),
                sort_text: Some(sort_text),
                kind: Some(CompletionItemKind::KEYWORD),
                documentation: symbol.info.documentation.as_ref().map(|doc| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: doc.clone(),
                    })
                }),
                ..Default::default()
            });
        }

        info!(
            "completion in {}ms: {} L{}C{} -> {} items",
            start.elapsed().as_millis(),
            &uri.path(),
            position.line + 1,
            position.character + 1,
            &items.len()
        );

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let doc = self.document_map.get(&uri.to_string()).unwrap();

        let mut code_actions = Vec::new();

        for diagnostic in params.context.diagnostics {
            let undefined_type_re =
                Regex::new(r"type referenced but not defined \(check namespace\): (\w+)").unwrap();
            let Some(captures) = undefined_type_re.captures(&diagnostic.message) else {
                continue;
            };
            let type_name = captures.get(1).unwrap().as_str();

            for symbol_entry in self.workspace.symbols.iter() {
                if symbol_entry.value().info.name == type_name {
                    let symbol = symbol_entry.value();
                    let Ok(symbol_path) = symbol.info.location.uri.to_file_path() else {
                        continue;
                    };
                    let Ok(current_path) = uri.to_file_path() else {
                        continue;
                    };
                    let Some(current_dir) = current_path.parent() else {
                        continue;
                    };
                    let Some(relative_path) = pathdiff::diff_paths(&symbol_path, &current_dir)
                    else {
                        continue;
                    };

                    let last_include_line = doc
                        .lines()
                        .enumerate()
                        .filter(|(_, line)| line.starts_with("include "))
                        .last()
                        .map(|(i, _)| i as u32);
                    let insert_line = last_include_line.map_or(0, |line| line + 1);

                    let text_edit = TextEdit {
                        range: Range::new(
                            Position::new(insert_line, 0),
                            Position::new(insert_line, 0),
                        ),
                        new_text: format!("include \"{}\";\n", relative_path.to_str().unwrap()),
                    };

                    let mut changes = HashMap::new();
                    changes.insert(uri.clone(), vec![text_edit]);

                    let code_action = CodeAction {
                        title: format!(
                            "Import `{}` from `{}`",
                            type_name,
                            relative_path.to_str().unwrap()
                        ),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diagnostic.clone()]),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    code_actions.push(CodeActionOrCommand::CodeAction(code_action));
                }
            }
        }

        Ok(Some(code_actions))
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        let logger = LspLogger::new(client.clone());
        if let Err(e) = log::set_boxed_logger(Box::new(logger)) {
            eprintln!("Error setting logger: {}", e);
        }
        log::set_max_level(log::LevelFilter::Debug);

        let mut workspace = Workspace::new();
        populate_builtins(&mut workspace);

        Backend {
            client,
            document_map: DashMap::new(),
            workspace,
            parser: FlatcFFIParser,
        }
    });

    info!("Starting server...");
    Server::new(stdin, stdout, socket).serve(service).await;
}
