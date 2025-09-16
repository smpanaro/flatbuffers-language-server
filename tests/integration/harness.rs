use flatbuffers_language_server::server::Backend;
use serde::de::DeserializeOwned;
use std::collections::VecDeque;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tower_lsp::jsonrpc::{Request, Response};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::request::Request as LspRequest;
use tower_lsp::lsp_types::*;
use tower_lsp::{LspService, Server};

// This file leverages code from:
// https://github.com/veryl-lang/veryl/blob/fdac1dfafff82e1227239b77930700927b091de1/crates/languageserver/src/tests.rs#L15

pub struct TestHarness {
    req_stream: DuplexStream,
    res_stream: DuplexStream,
    responses: VecDeque<String>,
    request_id: i64,
}

impl TestHarness {
    pub fn new() -> Self {
        let (req_client, req_server) = io::duplex(1024);
        let (res_server, res_client) = io::duplex(1024);

        let (service, socket) = LspService::new(Backend::new);

        tokio::spawn(Server::new(req_server, res_server, socket).serve(service));

        Self {
            req_stream: req_client,
            res_stream: res_client,
            responses: VecDeque::new(),
            request_id: 0,
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

    async fn recv_response(&mut self) -> Response {
        while self.responses.is_empty() {
            let mut buf = vec![0; 1024];
            let n = self.res_stream.read(&mut buf).await.unwrap();
            if n == 0 {
                panic!("server closed");
            }
            let ret = String::from_utf8(buf[..n].to_vec()).unwrap();
            for x in Self::decode(&ret) {
                self.responses.push_front(x);
            }
        }

        loop {
            let res = self.responses.pop_back().unwrap();
            if res.contains("id") {
                return serde_json::from_str(&res).unwrap();
            } else {
                self.responses.push_front(res);
            }
        }
    }

    async fn recv_notification_req(&mut self) -> Request {
        if self.responses.is_empty() {
            let mut buf = vec![0; 1024];
            let n = self.res_stream.read(&mut buf).await.unwrap();
            let ret = String::from_utf8(buf[..n].to_vec()).unwrap();
            for x in Self::decode(&ret) {
                self.responses.push_front(x);
            }
        }
        let res = self.responses.pop_back().unwrap();
        serde_json::from_str(&res).unwrap()
    }

    fn next_request_id(&mut self) -> i64 {
        self.request_id += 1;
        self.request_id
    }

    pub async fn initialize_and_open(&mut self, workspace: &[(&str, &str)]) {
        let id = self.next_request_id();
        let params = InitializeParams::default();
        let req = Request::build("initialize")
            .params(serde_json::to_value(params).unwrap())
            .id(id)
            .finish();
        self.send_request(req).await;
        let res = self.recv_response().await;
        assert!(res.is_ok());

        let params = InitializedParams {};
        let req = Request::build("initialized")
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send_request(req).await;

        for (name, content) in workspace {
            let uri = Url::from_file_path(format!("/{}", name)).unwrap();
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
        let res = self.recv_response().await;
        serde_json::from_value(res.result().unwrap().clone()).unwrap()
    }

    pub async fn notification<N: Notification>(&mut self) -> N::Params
    where
        N::Params: DeserializeOwned,
    {
        let req = self.recv_notification_req().await;
        assert_eq!(req.method(), N::METHOD);
        serde_json::from_value(req.params().unwrap().clone()).unwrap()
    }
}
