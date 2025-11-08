use log::{Level, Log, Metadata, Record};
use tower_lsp_server::lsp_types::MessageType;
use tower_lsp_server::Client;

fn level_to_message_type(level: Level) -> MessageType {
    match level {
        Level::Error => MessageType::ERROR,
        Level::Warn => MessageType::WARNING,
        Level::Info => MessageType::INFO,
        Level::Debug | Level::Trace => MessageType::LOG,
    }
}

#[derive(Debug)]
pub struct LspLogger {
    client: Client,
}

impl LspLogger {
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self { client }
    }
}

impl Log for LspLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let client = self.client.clone();
            let message_type = level_to_message_type(record.level());
            let message = format!("[{}] {}", record.target(), record.args());

            // Silence noisy 3rd party crates.
            if record.target().contains("ignore") || record.target().contains("glob") {
                return;
            }

            // Spawn a task to send the log message to the client
            tokio::spawn(async move {
                client.log_message(message_type, message).await;
            });
        }
    }

    fn flush(&self) {}
}
