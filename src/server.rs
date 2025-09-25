use crate::handlers::{code_action, completion, goto_definition, hover, lifecycle, references};
use crate::parser::{FlatcFFIParser, Parser};
use crate::workspace::Workspace;
use dashmap::DashMap;
use log::{error, info};
use ropey::Rope;
use std::collections::HashSet;
use std::time::Instant;
use tokio::fs;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, Location,
    OneOf, ReferenceParams, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, Url,
};
use tower_lsp::{Client, LanguageServer};

#[derive(Debug, Clone)]
pub struct Backend {
    pub client: Client,
    pub document_map: DashMap<String, Rope>,
    pub workspace: Workspace,
    pub parser: FlatcFFIParser,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: DashMap::new(),
            workspace: Workspace::new(),
            parser: FlatcFFIParser,
        }
    }

    // TODO: Move this to workspace
    pub async fn parse_and_discover(&self, initial_uri: Url, initial_content: Option<String>) {
        let mut files_to_parse = vec![(initial_uri.clone(), initial_content)];
        let mut parsed_files = HashSet::new();
        let mut all_diagnostics = std::collections::HashMap::new();

        let old_included_files = self
            .workspace
            .file_includes
            .get(&initial_uri)
            .map(|v| v.value().clone())
            .unwrap_or_default();

        while let Some((uri, content_opt)) = files_to_parse.pop() {
            if !parsed_files.insert(uri.clone()) {
                continue;
            }

            let content = if let Some(c) = content_opt {
                self.document_map
                    .insert(uri.to_string(), Rope::from_str(&c));
                c
            } else if let Some(doc) = self.document_map.get(&uri.to_string()) {
                doc.value().to_string()
            } else {
                match fs::read_to_string(uri.to_file_path().unwrap()).await {
                    Ok(text) => {
                        self.document_map
                            .insert(uri.to_string(), Rope::from_str(&text));
                        text
                    }
                    Err(e) => {
                        error!("Failed to read file {}: {}", uri, e);
                        continue;
                    }
                }
            };

            let start_time = Instant::now();
            let (diagnostics_map, symbol_table, included_files, root_type_info) =
                self.parser.parse(&uri, &content);
            let elapsed_time = start_time.elapsed();
            error!("Parsed in {}ms: {}", elapsed_time.as_millis(), uri);

            if let Some(st) = symbol_table {
                self.workspace
                    .update_symbols(&uri, st, included_files.clone(), root_type_info);
            } else {
                self.workspace.update_includes(&uri, included_files.clone());
            }

            for (file_uri, diagnostics) in diagnostics_map {
                all_diagnostics.insert(file_uri, diagnostics);
            }

            for included_path_str in included_files {
                match Url::from_file_path(&included_path_str) {
                    Ok(included_uri) => {
                        if !parsed_files.contains(&included_uri) {
                            files_to_parse.push((included_uri, None));
                        }
                    }
                    Err(_) => {
                        error!("Invalid include path: {}", included_path_str);
                    }
                }
            }
        }

        let mut files_to_update = HashSet::new();
        files_to_update.insert(initial_uri);
        for path in old_included_files {
            if let Ok(uri) = Url::from_file_path(&path) {
                files_to_update.insert(uri);
            }
        }
        for uri in parsed_files {
            files_to_update.insert(uri);
        }

        for uri in files_to_update {
            let diagnostics = all_diagnostics.remove(&uri).unwrap_or_default();
            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }
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
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        will_save: Some(false),
                        will_save_wait_until: Some(false),
                        save: Some(true.into()),
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ":".to_string(),
                        " ".to_string(),
                        "(".to_string(),
                        ",".to_string(),
                    ]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    completion_item: None,
                }),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![CodeActionKind::QUICKFIX]),
                        ..CodeActionOptions::default()
                    },
                )),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Server initialized!");
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down server...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        lifecycle::handle_did_open(self, params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        lifecycle::handle_did_change(self, params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        lifecycle::handle_did_close(self, params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        lifecycle::handle_did_save(self, params).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        hover::handle_hover(self, params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        goto_definition::handle_goto_definition(self, params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        references::handle_references(self, params).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        completion::handle_completion(self, params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        code_action::handle_code_action(self, params).await
    }
}
