use tower_lsp_server::lsp_types::{request::Request, DidSaveTextDocumentParams};

#[derive(Debug)]
pub enum DidSaveSync {}

impl Request for DidSaveSync {
    type Params = DidSaveTextDocumentParams;
    type Result = i32; // Can't be empty otherwise it will be treated as a notification.
    const METHOD: &'static str = "test/didSaveSync";
}
