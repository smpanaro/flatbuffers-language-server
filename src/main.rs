use dashmap::DashMap;
use flexi_logger::{FileSpec, Logger, WriteMode};
use log::info;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod parser;

#[derive(Debug)]
struct Backend {
    client: Client,
    document_map: DashMap<String, String>,
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
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Server initialized!")
            .await;
        info!("Server initialized!");
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down server...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        info!("Opened file: {}", params.text_document.uri);
        self.document_map.insert(
            params.text_document.uri.to_string(),
            params.text_document.text,
        );
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        info!("Changed file: {}", params.text_document.uri);
        self.document_map.insert(
            params.text_document.uri.to_string(),
            params.content_changes.remove(0).text,
        );
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        info!("Closed file: {}", params.text_document.uri);
        self.document_map
            .remove(&params.text_document.uri.to_string());
    }
}

#[tokio::main]
async fn main() {
    let _logger = Logger::try_with_str("info")
        .unwrap()
        .log_to_file(FileSpec::default().basename("fbs-lsp"))
        .write_mode(WriteMode::BufferAndFlush)
        .start()
        .unwrap();

    info!("Starting server...");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        document_map: DashMap::new(),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
