use dashmap::DashMap;
use log::{debug, error, info};
use std::collections::HashSet;
use std::time::Instant;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{OneOf, *};
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::ext::range::RangeExt;
use crate::lsp_logger::LspLogger;
use crate::parser::{FlatcFFIParser, Parser};
use crate::symbol_table::SymbolTable;
use tokio::fs;

mod ext;
mod ffi;
mod lsp_logger;
mod parser;
mod symbol_table;
mod utils;

#[derive(Debug)]
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
    // TODO: This is definitely the wrong data structure since flatc parses all included files automatically.
    symbol_map: DashMap<String, SymbolTable>,
    parser: FlatcFFIParser,
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
            let (diagnostics, symbol_table, included_files) = self.parser.parse(&uri, &content);
            let elapsed_time = start_time.elapsed();
            error!("Parsed in {}ms: {}", elapsed_time.as_millis(), uri);

            if let Some(st) = symbol_table {
                self.symbol_map.insert(uri.to_string(), st);
            } else {
                self.symbol_map.remove(&uri.to_string());
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

    async fn on_change(&self, uri: Url, text: String) {
        self.parse_and_discover(uri, Some(text)).await;
    }

    async fn on_hover(&self, uri: &Url, position: Position) -> Result<Option<Hover>> {
        let Some(st) = self.symbol_map.get(&uri.to_string()) else {
            return Ok(None);
        };

        let Some(symbol) = st.value().find_in_table(uri.clone(), position) else {
            return Ok(None);
        };

        if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
            for variant in &u.variants {
                if variant.location.range.contains(position) {
                    let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                    if let Some(variant_type_sym) = self
                        .symbol_map
                        .iter()
                        .find_map(|st| st.value().get(base_name).cloned())
                    {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: variant_type_sym.hover_markdown(),
                            }),
                            range: Some(variant.location.range),
                        }));
                    }
                    return Ok(None); // builtins
                }
            }
        }

        if let symbol_table::SymbolKind::Field(f) = &symbol.kind {
            let inner_type_range =
                utils::type_utils::calculate_inner_type_range(f.type_range, &f.type_name);
            if inner_type_range.contains(position) {
                let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
                if let Some(field_type_sym) = self
                    .symbol_map
                    .iter()
                    .find_map(|st| st.value().get(base_type_name).cloned())
                {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: field_type_sym.hover_markdown(),
                        }),
                        range: Some(inner_type_range),
                    }));
                }
                return Ok(None); // builtins
            }
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: symbol.hover_markdown(),
            }),
            range: Some(symbol.info.location.range),
        }))
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
        self.on_change(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        debug!("Changed: {}", params.text_document.uri);
        self.on_change(
            params.text_document.uri,
            params.content_changes.remove(0).text,
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("closed: {}", params.text_document.uri);
        self.document_map
            .remove(&params.text_document.uri.to_string());
        self.symbol_map
            .remove(&params.text_document.uri.to_string());
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let start = Instant::now();

        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let res = self.on_hover(&uri, pos).await;

        let elapsed = start.elapsed();
        info!(
            "hover in {}ms: {} L{}C{}",
            elapsed.as_millis(),
            &uri.path(),
            pos.line + 1,
            pos.character + 1
        );
        return res;
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(st) = self.symbol_map.get(&uri.to_string()) else {
            return Ok(None);
        };

        let Some(symbol) = st.value().find_in_table(uri, position) else {
            return Ok(None);
        };

        if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
            for variant in &u.variants {
                if variant.location.range.contains(position) {
                    let base_name = utils::type_utils::extract_base_type_name(&variant.name);
                    if let Some(variant_type_sym) = self
                        .symbol_map
                        .iter()
                        .find_map(|st| st.value().get(base_name).cloned())
                    {
                        return Ok(Some(GotoDefinitionResponse::Scalar(
                            variant_type_sym.info.location.clone(),
                        )));
                    }
                    return Ok(None); // builtins
                }
            }
        }

        if let symbol_table::SymbolKind::Field(f) = &symbol.kind {
            let inner_type_range =
                utils::type_utils::calculate_inner_type_range(f.type_range, &f.type_name);
            if inner_type_range.contains(position) {
                let base_type_name = utils::type_utils::extract_base_type_name(&f.type_name);
                if let Some(field_type_sym) = self
                    .symbol_map
                    .iter()
                    .find_map(|st| st.value().get(base_type_name).cloned())
                {
                    return Ok(Some(GotoDefinitionResponse::Scalar(
                        field_type_sym.info.location.clone(),
                    )));
                }
                return Ok(None); // builtins
            }
        }

        Ok(Some(GotoDefinitionResponse::Scalar(
            symbol.info.location.clone(),
        )))
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

        Backend {
            client,
            document_map: DashMap::new(),
            symbol_map: DashMap::new(),
            parser: FlatcFFIParser,
        }
    });

    info!("Starting server...");
    Server::new(stdin, stdout, socket).serve(service).await;
}
