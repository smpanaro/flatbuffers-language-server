use dashmap::DashMap;
use log::info;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use crate::lsp_logger::LspLogger;
use crate::parser::{FlatcFFIParser, Parser};
use crate::symbol_table::SymbolTable;

mod ext;
mod ffi;
mod lsp_logger;
mod parser;
mod symbol_table;

#[derive(Debug)]
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
    symbol_map: DashMap<String, SymbolTable>,
    parser: FlatcFFIParser,
}

impl Backend {
    async fn on_change(&self, uri: Url, text: String) {
        self.document_map.insert(uri.to_string(), text.clone());

        let (diagnostics, symbol_table) = self.parser.parse(&uri, &text);

        if let Some(st) = symbol_table {
            info!("Successfully built symbol table for {}", uri);
            self.symbol_map.insert(uri.to_string(), st);
        } else {
            self.symbol_map.remove(&uri.to_string());
        }

        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    async fn on_hover(&self, uri: Url, position: Position) -> Result<Option<Hover>> {
        // Look for a hovered type (will always be the same file) e.g. hovering Foo in `table Foo {}`
        if let Some(symbol) = self
            .symbol_map
            .get(&uri.to_string())
            .and_then(|st| st.value().find_in_table(position).cloned())
        {
            if let symbol_table::SymbolKind::Field(f) = &symbol.kind {
                // Look up again, could be in any file.
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
            } else {
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: symbol.hover_markdown(),
                    }),
                    range: Some(symbol.info.location.range),
                }));
            }
        }
        // Check enum types(?), union types etc e.g. hovering Foo in `union Bar { Foo }`

        Ok(None)
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
        return self
            .on_hover(
                params.text_document_position_params.text_document.uri,
                params.text_document_position_params.position,
            )
            .await;
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
