use crate::lsp_logger::LspLogger;
use crate::parser::FlatcFFIParser;
use crate::server::Backend;
use crate::workspace::Workspace;
use dashmap::DashMap;
use log::info;
use tower_lsp::{LspService, Server};

mod analysis;
mod ext;
mod ffi;
mod handlers;
mod lsp_logger;
mod parser;
mod server;
mod symbol_table;
mod utils;
mod workspace;

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
            workspace: Workspace::new(),
            parser: FlatcFFIParser,
        }
    });

    info!("Starting server...");
    Server::new(stdin, stdout, socket).serve(service).await;
}
