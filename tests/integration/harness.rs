use flatbuffers_language_server::server::Backend;
use serde::de::DeserializeOwned;
use std::collections::VecDeque;
use std::fs;
use tempfile::TempDir;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::jsonrpc::{Id, Request, Response};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::request::Request as LspRequest;
use tower_lsp::lsp_types::*;
use tower_lsp::{LspService, Server};

use super::test_logger;

// This file leverages code from:
// https://github.com/veryl-lang/veryl/blob/fdac1dfafff82e1227239b77930700927b091de1/crates/languageserver/src/tests.rs#L15

#[derive(Debug)]
pub enum ServerMessage {
    Response(Response),
    Notification(Request),
}

pub struct TestHarness {
    req_stream: DuplexStream,
    res_stream: DuplexStream,
    read_buffer: Vec<u8>,
    responses: VecDeque<String>,
    unhandled_notifications: VecDeque<Request>,
    request_id: i64,
    #[allow(dead_code)] // Unused, but keep so the directory isn't cleaned up.
    temp_dir: TempDir,
    pub root_uri: Url,
}

impl TestHarness {
    pub fn new() -> Self {
        test_logger::init();
        let (req_client, req_server) = io::duplex(1024);
        let (res_server, res_client) = io::duplex(1024);

        let (service, socket) = LspService::new(Backend::new);

        tokio::spawn(Server::new(req_server, res_server, socket).serve(service));

        let temp_dir = TempDir::new().unwrap();
        let root_uri = Url::from_directory_path(temp_dir.path().canonicalize().unwrap()).unwrap();

        Self {
            req_stream: req_client,
            res_stream: res_client,
            read_buffer: Vec::new(),
            responses: VecDeque::new(),
            unhandled_notifications: VecDeque::new(),
            request_id: 0,
            temp_dir,
            root_uri,
        }
    }

    fn encode(payload: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload)
    }

    async fn send_request(&mut self, req: Request) {
        let req = serde_json::to_string(&req).unwrap();
        let req = Self::encode(&req);
        self.req_stream.write_all(req.as_bytes()).await.unwrap();
    }

    async fn recv_message(&mut self) -> ServerMessage {
        // Loop until we have successfully parsed at least one message.
        while self.responses.is_empty() {
            // fill_buffer now just reads bytes without trying to interpret them.
            if self.fill_buffer().await.is_err() {
                // Handle the error, e.g., the stream was closed.
                panic!("Failed to read from server");
            }

            // Now, try to parse messages from our persistent buffer.
            loop {
                let buf_str = String::from_utf8_lossy(&self.read_buffer);
                if let Some(p) = buf_str.find("\r\n\r\n") {
                    let header_end = p + 4;
                    let header = &buf_str[..p];

                    // Extract Content-Length
                    let len_str = header
                        .strip_prefix("Content-Length: ")
                        .expect("Missing Content-Length header");
                    let len: usize = len_str.parse().expect("Invalid Content-Length value");

                    let message_end = header_end + len;

                    // If we don't have the full message yet, break and wait for more data.
                    if self.read_buffer.len() < message_end {
                        break;
                    }

                    // We have a full message, so we can process it.
                    let message_bytes = &self.read_buffer[header_end..message_end];
                    let msg_str = String::from_utf8(message_bytes.to_vec())
                        .expect("Server sent invalid UTF-8");

                    self.responses.push_back(msg_str);

                    // IMPORTANT: Remove the consumed message from the buffer.
                    self.read_buffer.drain(..message_end);
                } else {
                    // No complete header found, wait for more data.
                    break;
                }
            }
        }

        let msg_str = self.responses.pop_front().unwrap();

        // Try to parse it as a Response. This works if an "id" field is present.
        if let Ok(response) = serde_json::from_str::<Response>(&msg_str) {
            return ServerMessage::Response(response);
        }

        // If that fails, try to parse it as a Notification (which looks like a Request with no id).
        if let Ok(notification) = serde_json::from_str::<Request>(&msg_str) {
            return ServerMessage::Notification(notification);
        }

        panic!("Failed to deserialize server message: {}", msg_str);
    }

    fn next_request_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    pub async fn initialize_and_open(&mut self, workspace: &[(&str, &str)]) {
        // 1. Write files to disk first so the server can see them during initialization.
        for (name, content) in workspace {
            let uri = self.root_uri.join(name).unwrap();
            let path = uri.to_file_path().unwrap();
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }

        // 2. Send "initialize" request.
        let mut params = InitializeParams::default();
        params.root_uri = Some(self.root_uri.clone());

        let id = self.next_request_id();
        let req = Request::build("initialize")
            .params(serde_json::to_value(params).unwrap())
            .id(id)
            .finish();
        self.send_request(req).await;
        let res = match self.recv_message().await {
            ServerMessage::Response(res) => res,
            ServerMessage::Notification(req) => {
                panic!(
                    "Received unexpected response while waiting for initizlie response: {:?}",
                    req
                );
            }
        };
        assert!(res.is_ok());

        // 3. Send "initialized" notification.
        let params = InitializedParams {};
        let req = Request::build("initialized")
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send_request(req).await;

        // 4. Send "didOpen" notifications for the files.
        for (name, content) in workspace {
            let uri = self.root_uri.join(name).unwrap();
            let text_document = TextDocumentItem {
                uri,
                language_id: "flatbuffers".to_string(),
                version: 1,
                text: content.to_string(),
            };
            let params = DidOpenTextDocumentParams { text_document };
            let req = Request::build("textDocument/didOpen")
                .params(serde_json::to_value(params).unwrap())
                .finish();
            self.send_request(req).await;
        }
    }

    pub async fn change_file(
        &mut self,
        identifier: VersionedTextDocumentIdentifier,
        content: &str,
    ) {
        if let Ok(path) = identifier.uri.to_file_path() {
            fs::write(path, content).unwrap();
        }

        let params = DidChangeTextDocumentParams {
            text_document: identifier,
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: content.to_string(),
            }],
        };
        let req = Request::build("textDocument/didChange")
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send_request(req).await;
    }

    pub async fn close_file(&mut self, uri: Url) {
        let params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        };
        let req = Request::build("textDocument/didClose")
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send_request(req).await;
    }

    pub async fn call<R: LspRequest>(&mut self, params: R::Params) -> R::Result
    where
        R::Result: DeserializeOwned,
    {
        let id = self.next_request_id();
        let req = Request::build(R::METHOD)
            .params(serde_json::to_value(params).unwrap())
            .id(id)
            .finish();
        self.send_request(req).await;

        loop {
            match self.recv_message().await {
                ServerMessage::Response(res) => {
                    // Check if this is the response we are waiting for.
                    if res.id() == &Id::Number(id) {
                        let value = res.result().expect("Request failed").clone();
                        return serde_json::from_value(value)
                            .expect("Failed to deserialize response result");
                    } else {
                        // This is a response for a different request. This shouldn't happen in a
                        // single-threaded test harness, so we'll panic.
                        panic!("Received response for unexpected request id. Expected: {:?}, Got: {:?}", id, res.id());
                    }
                }
                ServerMessage::Notification(req) => {
                    // We received a notification while waiting for a response.
                    // Store it in our buffer to be processed by a later call to `notification()`.
                    self.unhandled_notifications.push_back(req);
                }
            }
        }
    }

    pub async fn notification<N: Notification>(&mut self) -> N::Params
    where
        N::Params: DeserializeOwned,
    {
        // First, check our buffer of unhandled notifications to see if we've already received the one we want.
        if let Some(pos) = self
            .unhandled_notifications
            .iter()
            .position(|req| req.method() == N::METHOD)
        {
            let req = self.unhandled_notifications.remove(pos).unwrap();
            let params = req
                .params()
                .expect("Notification is missing params")
                .clone();
            return serde_json::from_value(params)
                .expect("Failed to deserialize notification params");
        }

        // If not, listen for new messages from the server.
        loop {
            match self.recv_message().await {
                ServerMessage::Response(res) => {
                    panic!(
                        "Received unexpected response while waiting for a notification: {:?}",
                        res
                    );
                }
                ServerMessage::Notification(req) => {
                    if req.method() == N::METHOD {
                        let params = req
                            .params()
                            .expect("Notification is missing params")
                            .clone();
                        return serde_json::from_value(params)
                            .expect("Failed to deserialize notification params");
                    } else {
                        // This is a different notification, so store it for later.
                        self.unhandled_notifications.push_back(req);
                    }
                }
            }
        }
    }

    async fn fill_buffer(&mut self) -> io::Result<()> {
        // Only read from the stream if our internal buffer of messages is empty.
        if !self.responses.is_empty() {
            return Ok(());
        }

        let mut buf = vec![0; 8192];
        // Use the timeout when reading from the socket.
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.res_stream.read(&mut buf),
        )
        .await
        {
            // Server closed the connection.
            Ok(Ok(0)) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "server closed connection",
            )),
            // We received some bytes.
            Ok(Ok(n)) => {
                self.read_buffer.extend_from_slice(&buf[..n]);
                Ok(())
            }
            // An I/O error occurred.
            Ok(Err(e)) => Err(e),
            // The read timed out.
            Err(_) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out waiting for a response",
            )),
        }
    }

    pub async fn wait_for_diagnostic(&mut self, message: &str) -> Option<Diagnostic> {
        loop {
            let params = self
                .notification::<notification::PublishDiagnostics>()
                .await;
            for diag in params.diagnostics {
                if diag.message.contains(message) {
                    return Some(diag);
                }
            }
        }
    }

    pub async fn get_first_diagnostic_for_file(&mut self, uri: &Url) -> Diagnostic {
        loop {
            let params = self
                .notification::<notification::PublishDiagnostics>()
                .await;
            if &params.uri == uri {
                if !params.diagnostics.is_empty() {
                    return params.diagnostics[0].clone();
                }
            }
        }
    }
}
