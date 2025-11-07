use crate::analysis::Analyzer;
use crate::document_store::DocumentStore;
#[cfg(any(test, feature = "test-harness"))]
use crate::ext::all_diagnostics::AllDiagnostics;
use crate::handlers::{
    code_action, completion, goto_definition, hover, lifecycle, references, rename,
    workspace_symbol,
};
#[cfg(any(test, feature = "test-harness"))]
use crate::utils::paths::path_buf_to_uri;
use log::{error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use tower_lsp_server::jsonrpc::Result;
#[cfg(any(test, feature = "test-harness"))]
use tower_lsp_server::lsp_types::request::Request;
use tower_lsp_server::lsp_types::request::WorkDoneProgressCreate;
use tower_lsp_server::lsp_types::{
    notification, CodeActionKind, CodeActionOptions, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWatchedFilesRegistrationOptions, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    FileSystemWatcher, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    Location, NumberOrString, OneOf, PrepareRenameResponse, ProgressParams, ProgressParamsValue,
    ReferenceParams, Registration, RenameOptions, RenameParams, ServerCapabilities, ServerInfo,
    SymbolInformation, TextDocumentPositionParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkspaceEdit,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use tower_lsp_server::{Client, LanguageServer};

#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub documents: Arc<DocumentStore>,
    pub analyzer: Arc<Analyzer>,
    // Initialize scan.
    ready: AtomicBool,
    notify_ready: Notify,
}

impl Backend {
    #[must_use] pub fn new(client: Client) -> Self {
        let documents = Arc::new(DocumentStore::new());
        let analysis = Arc::new(Analyzer::new(Arc::clone(&documents)));
        Self {
            client,
            documents,
            analyzer: analysis,
            ready: AtomicBool::new(false),
            notify_ready: Notify::new(),
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        info!("Initializing server...");
        info!("PID: {}", std::process::id());
        lifecycle::handle_initialize(self, params).await;

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "flatbuffers-language-server".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
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
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Right(
                            "flatbuffers-language-server-workspace-folders".to_string(),
                        )),
                    }),
                    // workspace/didChangeWatchedFiles is more robust since
                    // it handles changes outside of the IDE.
                    file_operations: None,
                }),
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
                        ".".to_string(),
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
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Client initialized!");

        let token = NumberOrString::String("initial-repo-scan".to_string());
        if let Err(err) = self
            .client
            .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                token: token.clone(),
            })
            .await
        {
            error!("failed to create initialized scan progress: {err}");
        }

        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                    WorkDoneProgressBegin {
                        title: "flatbuffers".to_string(),
                        cancellable: Some(false),
                        message: Some("discovering files".to_string()),
                        percentage: None,
                    },
                )),
            })
            .await;

        let diagnostics = lifecycle::handle_initialized(self).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
        self.mark_ready();

        self.client
            .send_notification::<notification::Progress>(ProgressParams {
                token: token.clone(),
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd {
                    message: Some("complete".to_string()),
                })),
            })
            .await;

        self.client
            .register_capability(vec![Registration {
                id: "fbs-watcher".to_string(),
                method: "workspace/didChangeWatchedFiles".to_string(),
                register_options: Some(
                    serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
                        watchers: vec![FileSystemWatcher {
                            glob_pattern: GlobPattern::String("**/*.fbs".to_string()),
                            kind: None, // None means all changes
                        }],
                    })
                    .unwrap(),
                ),
            }])
            .await
            .unwrap();

        info!("Server initialized!");
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down server...");
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.wait_until_ready().await;
        let diagnostics = lifecycle::handle_did_open(self, params).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.wait_until_ready().await;
        let diagnostics = lifecycle::handle_did_change(self, params).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.wait_until_ready().await;
        lifecycle::handle_did_close(self, params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.wait_until_ready().await;
        let diagnostics = lifecycle::handle_did_save(self, params).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.wait_until_ready().await;
        let diagnostics = self.analyzer.handle_file_changes(params.changes).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        self.wait_until_ready().await;
        let diagnostics = lifecycle::handle_did_change_workspace_folders(self, params).await;
        for (uri, diags) in diagnostics {
            self.client.publish_diagnostics(uri, diags, None).await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        hover::handle_hover(&snapshot, params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        goto_definition::handle_goto_definition(&snapshot, params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        references::handle_references(&snapshot, params).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        completion::handle_completion(&snapshot, params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        code_action::handle_code_action(&snapshot, params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        rename::prepare_rename(&snapshot, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        rename::rename(&snapshot, params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        self.wait_until_ready().await;
        let snapshot = self.analyzer.snapshot().await;
        let result = workspace_symbol::handle_workspace_symbol(&snapshot, params).await?;
        Ok(result.map(OneOf::Right))
    }
}

impl Backend {
    async fn wait_until_ready(&self) {
        if self.ready.load(Ordering::Acquire) {
            return;
        }
        self.notify_ready.notified().await;
    }

    fn mark_ready(&self) {
        self.ready.store(true, Ordering::Release);
        self.notify_ready.notify_waiters();
    }
}

#[cfg(any(test, feature = "test-harness"))]
impl Backend {
    pub async fn did_save_sync(&self, params: DidSaveTextDocumentParams) -> Result<i32> {
        self.did_save(params).await;
        Ok(0)
    }

    pub async fn all_diagnostics(
        &self,
        _: <AllDiagnostics as Request>::Params,
    ) -> Result<<AllDiagnostics as Request>::Result> {
        let snapshot = self.analyzer.snapshot().await;
        let diagnostics = snapshot.diagnostics.all();
        let result = diagnostics
            .iter()
            .filter_map(|(path, diags)| path_buf_to_uri(path).ok().map(|uri| (uri, diags.clone())))
            .collect();
        Ok(result)
    }
}
