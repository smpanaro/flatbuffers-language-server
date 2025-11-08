use tower_lsp_server::lsp_types::{
    request::Request, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, InitializedParams,
};

#[derive(Debug)]
pub enum DidSaveSync {}

impl Request for DidSaveSync {
    type Params = DidSaveTextDocumentParams;
    type Result = i32;
    const METHOD: &'static str = "test/didSaveSync";
}

#[derive(Debug)]
pub enum DidOpenSync {}

impl Request for DidOpenSync {
    type Params = DidOpenTextDocumentParams;
    type Result = i32;
    const METHOD: &'static str = "test/didOpenSync";
}

#[derive(Debug)]
pub enum DidChangeSync {}

impl Request for DidChangeSync {
    type Params = DidChangeTextDocumentParams;
    type Result = i32;
    const METHOD: &'static str = "test/didChangeSync";
}

#[derive(Debug)]
pub enum InitializedSync {}

impl Request for InitializedSync {
    type Params = InitializedParams;
    type Result = i32;
    const METHOD: &'static str = "test/initializedSync";
}
