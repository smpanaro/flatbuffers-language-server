use crate::utils::paths::{is_flatbuffer_schema, uri_to_path_buf};
use dashmap::DashMap;
use log::debug;
use ropey::Rope;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams,
};

#[derive(Debug)]
pub struct DocumentStore {
    pub document_map: DashMap<PathBuf, Rope>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            document_map: DashMap::new(),
        }
    }

    pub fn handle_did_open(&self, params: DidOpenTextDocumentParams) -> Option<PathBuf> {
        debug!("opened: {}", params.text_document.uri.path());
        if !is_flatbuffer_schema(&params.text_document.uri) {
            return None;
        }
        let path = uri_to_path_buf(&params.text_document.uri).ok()?;

        self.document_map.insert(
            path.clone(),
            ropey::Rope::from_str(&params.text_document.text),
        );
        Some(path)
    }

    pub fn handle_did_change(&self, mut params: DidChangeTextDocumentParams) -> Option<PathBuf> {
        debug!("changed: {}", params.text_document.uri.path());
        if !is_flatbuffer_schema(&params.text_document.uri) {
            return None;
        }
        let path = uri_to_path_buf(&params.text_document.uri).ok()?;

        let content = params.content_changes.remove(0).text;
        self.document_map
            .insert(path.clone(), ropey::Rope::from_str(&content));
        Some(path)
    }

    pub fn handle_did_save(&self, params: DidSaveTextDocumentParams) -> Option<(PathBuf, bool)> {
        debug!("saved: {}", params.text_document.uri.path());
        if !is_flatbuffer_schema(&params.text_document.uri) {
            return None;
        }
        let path = uri_to_path_buf(&params.text_document.uri).ok()?;

        let mut was_changed = false;
        if let Some(text) = params.text {
            self.document_map
                .insert(path.clone(), ropey::Rope::from_str(&text));
            was_changed = true;
        }
        Some((path, was_changed))
    }

    pub fn handle_did_close(&self, params: DidCloseTextDocumentParams) {
        debug!("closed: {}", params.text_document.uri.path());
        if !is_flatbuffer_schema(&params.text_document.uri) {
            return;
        }
    }
}
