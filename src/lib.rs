use crate::lsp_logger::LspLogger;
use crate::server::Backend;
use log::info;
use tower_lsp_server::{LspService, Server};

pub mod analysis;
pub mod diagnostics;
pub mod ext;
pub mod ffi;
pub mod handlers;
pub mod lsp_logger;
pub mod parser;
pub mod server;
pub mod symbol_table;
pub mod utils;
pub mod workspace;

pub async fn run() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        let logger = LspLogger::new(client.clone());
        if let Err(e) = log::set_boxed_logger(Box::new(logger)) {
            eprintln!("Error setting logger: {}", e);
        }
        log::set_max_level(log::LevelFilter::Debug);

        Backend::new(client)
    });

    info!("Starting server v{}...", env!("CARGO_PKG_VERSION"));
    Server::new(stdin, stdout, socket).serve(service).await;
}
