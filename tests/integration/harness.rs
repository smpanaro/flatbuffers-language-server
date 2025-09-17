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
    responses: VecDeque<String>,
    unhandled_notifications: VecDeque<Request>,
    request_id: i64,
    #[allow(dead_code)] // Unused, but keep so the directory isn't cleaned up.
    temp_dir: TempDir,
    pub root_uri: Url,
}

impl TestHarness {
    pub fn new() -> Self {
        let (req_client, req_server) = io::duplex(1024);
        let (res_server, res_client) = io::duplex(1024);

        let (service, socket) = LspService::new(Backend::new);

        tokio::spawn(Server::new(req_server, res_server, socket).serve(service));

        let temp_dir = TempDir::new().unwrap();
        let root_uri = Url::from_directory_path(temp_dir.path().canonicalize().unwrap()).unwrap();

        Self {
            req_stream: req_client,
            res_stream: res_client,
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

    fn decode(text: &str) -> Vec<String> {
        let mut ret = Vec::new();
        let mut temp = text;

        while !temp.is_empty() {
            let p = temp.find("\r\n\r\n").unwrap();
            let (header, body) = temp.split_at(p + 4);
            let len = header
                .strip_prefix("Content-Length: ")
                .unwrap()
                .strip_suffix("\r\n\r\n")
                .unwrap();
            let len: usize = len.parse().unwrap();
            let (body, rest) = body.split_at(len);
            ret.push(body.to_string());
            temp = rest;
        }

        ret
    }

    async fn send_request(&mut self, req: Request) {
        let req = serde_json::to_string(&req).unwrap();
        let req = Self::encode(&req);
        self.req_stream.write_all(req.as_bytes()).await.unwrap();
    }

    async fn recv_message(&mut self) -> ServerMessage {
        // Ensure our buffer has messages to process.
        if self.responses.is_empty() {
            self.fill_buffer()
                .await
                .expect("Failed to read from server");
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

        let params = InitializedParams {};
        let req = Request::build("initialized")
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send_request(req).await;

        for (name, content) in workspace {
            let uri = self.root_uri.join(name).unwrap();
            fs::write(uri.path(), content).unwrap();

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
                let text = String::from_utf8(buf[..n].to_vec()).expect("server sent invalid UTF-8");
                for msg in Self::decode(&text) {
                    // Add new messages to the back of the queue.
                    self.responses.push_back(msg);
                }
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
}
