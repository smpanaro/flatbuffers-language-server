use dashmap::DashMap;
use log::info;
use std::collections::HashSet;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
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

#[derive(Debug)]
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
    // TODO: This may not be the correct data structure since flatc parses all included files automatically.
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
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!("Failed to read file {}: {}", uri, e),
                            )
                            .await;
                        continue;
                    }
                }
            };

            self.document_map.insert(uri.to_string(), content.clone());

            let (diagnostics, symbol_table, included_files) = self.parser.parse(&uri, &content);

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
                        self.client
                            .log_message(
                                MessageType::ERROR,
                                format!("Invalid include path: {}", included_path_str),
                            )
                            .await;
                    }
                }
            }
        }
    }

    async fn on_change(&self, uri: Url, text: String) {
        self.parse_and_discover(uri, Some(text)).await;
    }

    async fn on_hover(&self, uri: Url, position: Position) -> Result<Option<Hover>> {
        let Some(st) = self.symbol_map.get(&uri.to_string()) else {
            return Ok(None);
        };

        let Some(symbol) = st.value().find_in_table(position) else {
            return Ok(None);
        };

        if let symbol_table::SymbolKind::Union(u) = &symbol.kind {
            for variant in &u.variants {
                if variant.location.range.contains(position) {
                    if let Some(variant_type_sym) = self
                        .symbol_map
                        .iter()
                        .find_map(|st| st.value().get(&variant.name).cloned())
                    {
                        return Ok(Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: variant_type_sym.hover_markdown(),
                            }),
                            range: Some(variant.location.range),
                        }));
                    }
                }
            }
        }

        if let symbol_table::SymbolKind::Field(f) = &symbol.kind {
            if f.type_range.contains(position) {
                if let Some(field_type_sym) = self
                    .symbol_map
                    .iter()
                    .find_map(|st| st.value().get(&f.type_name).cloned())
                {
                    return Ok(Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: field_type_sym.hover_markdown(),
                        }),
                        range: Some(f.type_range),
                    }));
                }
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
        info!("Opened file: {}", params.text_document.uri);
        self.on_change(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        info!("Changed file: {}", params.text_document.uri);
        self.on_change(
            params.text_document.uri,
            params.content_changes.remove(0).text,
        )
        .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        info!("Closed file: {}", params.text_document.uri);
        self.document_map
            .remove(&params.text_document.uri.to_string());
        self.symbol_map
            .remove(&params.text_document.uri.to_string());
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        self.on_hover(
            params.text_document_position_params.text_document.uri,
            params.text_document_position_params.position,
        )
        .await
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
