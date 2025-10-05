use crate::ext::duration::DurationFormat;
use crate::handlers::{
    code_action, completion, goto_definition, hover, lifecycle, references, rename,
};
use crate::parser::{FlatcFFIParser, Parser};
use crate::utils::paths::{is_flatbuffer_schema, is_flatbuffer_schema_path};
use crate::workspace::Workspace;
use dashmap::{DashMap, DashSet};
use ignore::{WalkBuilder, WalkState};
use log::{debug, error, info};
use ropey::Rope;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;
use tokio::fs;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    CodeActionResponse, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidChangeWatchedFilesRegistrationOptions, DidChangeWorkspaceFoldersParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    FileSystemWatcher, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverParams, HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    Location, OneOf, PrepareRenameResponse, ReferenceParams, Registration, RenameOptions,
    RenameParams, ServerCapabilities, ServerInfo, TextDocumentPositionParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, Url, WorkspaceEdit,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp::{Client, LanguageServer};

#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    pub document_map: DashMap<String, Rope>,
    pub workspace: Workspace,
    pub search_paths: RwLock<Vec<Url>>,
    pub workspace_roots: DashSet<PathBuf>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: DashMap::new(),
            workspace: Workspace::new(),
            search_paths: RwLock::new(vec![]),
            workspace_roots: DashSet::new(),
        }
    }

    pub async fn update_search_paths(&self) {
        let start = Instant::now();
        let roots: Vec<_> = self
            .workspace_roots
            .iter()
            .map(|r| r.key().clone())
            .collect();

        if roots.is_empty() {
            let mut search_paths_guard = self.search_paths.write().await;
            *search_paths_guard = vec![];
            return;
        }

        let search_paths = DashSet::new();
        let mut builder = WalkBuilder::new(&roots[0]);
        if roots.len() > 1 {
            for root in &roots[1..] {
                builder.add(root);
            }
        }

        let roots_arc = std::sync::Arc::new(roots);
        builder.build_parallel().run(|| {
            let search_paths = &search_paths;
            let roots = std::sync::Arc::clone(&roots_arc);
            Box::new(move |result| {
                if let Ok(entry) = result {
                    if is_flatbuffer_schema_path(entry.path()) {
                        let intermediate_paths =
                            crate::utils::paths::get_intermediate_paths(entry.path(), &roots);
                        for path in intermediate_paths {
                            if let Ok(url) = Url::from_directory_path(&path) {
                                search_paths.insert(url);
                            }
                        }
                    }
                }
                WalkState::Continue
            })
        });

        let search_paths: Vec<Url> = search_paths.into_iter().collect();
        debug!(
            "discovered include paths in {}: {:?}",
            start.elapsed().log_str(),
            search_paths.iter().map(|u| u.path()).collect::<Vec<_>>()
        );

        let mut search_paths_guard = self.search_paths.write().await;
        *search_paths_guard = search_paths;
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
                        error!("failed to read file {}: {}", uri.path(), e);
                        continue;
                    }
                }
            };

            let start_time = Instant::now();
            let search_paths_guard = self.search_paths.read().await;

            // FlatcFFIParser is stateless.
            let (diagnostics_map, symbol_table, included_files, root_type_info) =
                FlatcFFIParser.parse(&uri, &content, &search_paths_guard);
            let elapsed_time = start_time.elapsed();
            debug!("parsed in {}: {}", elapsed_time.log_str(), uri.path());

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
                        error!("invalid include path: {}", included_path_str);
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
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        info!("Initializing server...");
        lifecycle::handle_initialize(&self, params).await;

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
                ..ServerCapabilities::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        info!("Server initialized!");

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

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        // What about folders? Watching files is sufficient.
        // New folder created     : empty so can't have .fbs files.
        // Existing folder deleted: if it has .fbs they will show up as deleted.
        // Existing folder renamed: if it has .fbs they will show up as deleted
        //                          and created in the new location.
        let should_update = params
            .changes
            .iter()
            .any(|event| is_flatbuffer_schema(&event.uri));

        if should_update {
            info!("fbs file changed, updating search paths...");
            // In theory we could avoid a full scan, but
            // these events are rare enough that we can stay simple.
            self.update_search_paths().await;
        }
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        lifecycle::handle_did_change_workspace_folders(self, params).await
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

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        rename::prepare_rename(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        rename::rename(self, params).await
    }
}
