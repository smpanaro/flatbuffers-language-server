use crate::ext::duration::DurationFormat;
use crate::handlers::{
    code_action, completion, goto_definition, hover, lifecycle, references, rename,
};
use crate::utils::paths::{
    get_intermediate_paths, is_flatbuffer_schema, is_flatbuffer_schema_path, path_buf_to_url,
    uri_to_path_buf,
};
use crate::workspace::Workspace;
use dashmap::{DashMap, DashSet};
use ignore::{WalkBuilder, WalkState};
use log::{debug, error, info};
use ropey::Rope;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::{Notify, RwLock};
use tower_lsp_server::jsonrpc::Result;
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
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, Uri, WorkDoneProgress, WorkDoneProgressBegin,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd, WorkspaceEdit,
    WorkspaceFoldersServerCapabilities, WorkspaceServerCapabilities,
};
use tower_lsp_server::{Client, LanguageServer, UriExt};

#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    // TODO: Some/all of this should move to backend.
    pub document_map: DashMap<PathBuf, Rope>,
    pub workspace: Workspace,
    pub search_paths: RwLock<Vec<PathBuf>>,
    pub workspace_roots: DashSet<PathBuf>,
    // Initialize scan.
    ready: AtomicBool,
    notify_ready: Notify,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            document_map: DashMap::new(),
            workspace: Workspace::new(),
            search_paths: RwLock::new(vec![]),
            workspace_roots: DashSet::new(),
            ready: AtomicBool::new(false),
            notify_ready: Notify::new(),
        }
    }

    pub async fn update_search_paths_and_discover_files(&self) -> Vec<PathBuf> {
        let start = Instant::now();
        let roots: Vec<_> = self
            .workspace_roots
            .iter()
            .map(|r| r.key().clone())
            .collect();

        if roots.is_empty() {
            let mut search_paths_guard = self.search_paths.write().await;
            *search_paths_guard = vec![];
            return vec![];
        }

        let search_paths = DashSet::new();
        let fbs_files = DashSet::new();

        let mut builder = WalkBuilder::new(&roots[0]);
        if roots.len() > 1 {
            for root in &roots[1..] {
                builder.add(root);
            }
        }

        let roots_arc = std::sync::Arc::new(roots);
        builder.build_parallel().run(|| {
            let search_paths = &search_paths;
            let fbs_files = &fbs_files;
            let roots = std::sync::Arc::clone(&roots_arc);
            Box::new(move |result| {
                if let Ok(entry) = result {
                    if is_flatbuffer_schema_path(entry.path()) {
                        if let Ok(path) = fs::canonicalize(entry.path()) {
                            fbs_files.insert(path.to_path_buf());

                            let intermediate_paths = get_intermediate_paths(&path, &roots);
                            for intermediate in intermediate_paths {
                                search_paths.insert(intermediate);
                            }
                        }
                    }
                }
                WalkState::Continue
            })
        });

        let search_paths: Vec<PathBuf> = search_paths.into_iter().collect();
        debug!(
            "discovered include paths in {}: {:?}",
            start.elapsed().log_str(),
            search_paths,
        );

        let mut search_paths_guard = self.search_paths.write().await;
        *search_paths_guard = search_paths;

        fbs_files.into_iter().collect::<Vec<_>>()
    }

    pub async fn scan_workspace(&self) {
        let fbs_files = self
            .update_search_paths_and_discover_files()
            .await
            .into_iter()
            .filter(|uri| !self.workspace.has_symbols_for(uri));
        self.parse_many_and_publish(fbs_files).await;
    }

    pub async fn remove_workspace_folder(&self, folder_uri: &Uri) {
        let Ok(folder_path) = uri_to_path_buf(folder_uri) else {
            return;
        };
        let uris_to_remove: HashSet<PathBuf> = self
            .workspace
            .file_definitions
            .iter()
            .map(|entry| entry.key().clone())
            .filter(|path| path.starts_with(&folder_path))
            .collect();

        let mut files_to_reparse = HashSet::new();
        for path in &uris_to_remove {
            let affected_files = self.workspace.remove_file(path);
            for f in affected_files {
                if !uris_to_remove.contains(&f) {
                    files_to_reparse.insert(f);
                }
            }

            if let Ok(uri) = path_buf_to_url(path) {
                self.client.publish_diagnostics(uri, vec![], None).await;
            }
        }

        let mut search_paths_guard = self.search_paths.write().await;
        search_paths_guard.retain(|path| !path.starts_with(&folder_path));
        drop(search_paths_guard);

        self.parse_many_and_publish(files_to_reparse).await;
    }

    pub async fn parse_many_and_publish(&self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut parsed_in_scan = HashSet::new();
        for path in paths {
            if !parsed_in_scan.contains(&path) {
                self.parse_and_publish(&path, &mut parsed_in_scan).await;
            }
        }
    }

    pub async fn parse_and_publish(&self, path: &PathBuf, parsed_in_scan: &mut HashSet<PathBuf>) {
        let search_paths_guard = self.search_paths.read().await;
        let (diagnostics, updated_files) = self
            .workspace
            .parse_and_update(
                path.to_path_buf(),
                &self.document_map,
                &search_paths_guard,
                parsed_in_scan,
            )
            .await;

        for file_path in updated_files {
            let mut new_diags = diagnostics.get(&file_path).cloned().unwrap_or_default();

            new_diags.sort_by(|a, b| {
                a.message
                    .cmp(&b.message)
                    .then_with(|| a.range.start.cmp(&b.range.start))
            });

            let old_diags = self.workspace.published_diagnostics.get(&file_path);

            let has_changed = old_diags.map_or(true, |d| *d.value() != new_diags);
            if !has_changed {
                continue;
            }

            if let Ok(file_uri) = path_buf_to_url(&file_path) {
                self.client
                    .publish_diagnostics(file_uri, new_diags.clone(), None)
                    .await;
                self.workspace
                    .published_diagnostics
                    .insert(file_path, new_diags);
            }
        }
    }
}

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
        info!("Client initialized!");

        let token = NumberOrString::String("initial-repo-scan".to_string());
        if let Err(err) = self
            .client
            .send_request::<WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                token: token.clone(),
            })
            .await
        {
            error!("failed to create initialized scan progress: {}", err)
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

        lifecycle::handle_initialized(&self).await;
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
        lifecycle::handle_did_open(self, params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.wait_until_ready().await;
        lifecycle::handle_did_change(self, params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.wait_until_ready().await;
        lifecycle::handle_did_close(self, params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.wait_until_ready().await;
        lifecycle::handle_did_save(self, params).await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        self.wait_until_ready().await;

        // What about folders? Watching files is sufficient.
        // New folder created     : empty so can't have .fbs files.
        // Existing folder deleted: if it has .fbs they will show up as deleted.
        // Existing folder renamed: if it has .fbs they will show up as deleted
        //                          and created in the new location.
        // ... except from VSCode. For which we handle folders below.

        let mut files_to_reparse = HashSet::new();

        for event in params.changes {
            // Canonicalize will fail for deleted files, so fall back to non-canonical.
            // TODO: Figure out a better heuristic (resolve parents or store client<>canonical map or scan).
            let non_canonical = event.uri.to_file_path().and_then(|p| Some(p.to_path_buf()));
            let Some(path) = uri_to_path_buf(&event.uri).ok().or(non_canonical) else {
                continue;
            };

            let has_ext = path.extension().is_some();
            if !is_flatbuffer_schema(&event.uri) && has_ext {
                continue;
            }

            match event.typ {
                tower_lsp_server::lsp_types::FileChangeType::CREATED => {
                    files_to_reparse.insert(path);
                }
                tower_lsp_server::lsp_types::FileChangeType::CHANGED => {
                    // NOTE: This doubles the work done on save,
                    // but allows us to capture file changes made
                    // outside of the client (e.g. git checkout).
                    files_to_reparse.insert(path);
                }
                tower_lsp_server::lsp_types::FileChangeType::DELETED => {
                    // VSCode doesn't report the files in a deleted folder, so we do our best.
                    let deleted_files = self.workspace.expand_to_known_files(&path);
                    for deleted in deleted_files.clone() {
                        let affected_files = self.workspace.remove_file(&deleted);
                        for uri in affected_files {
                            if !deleted_files.contains(&uri) {
                                files_to_reparse.insert(uri);
                            }
                        }
                        info!("marking {} deleted", deleted.display());
                        self.document_map.remove(&deleted);
                        if let Ok(deleted_uri) = path_buf_to_url(&deleted) {
                            // TODO: Should also update the cache.
                            self.client
                                .publish_diagnostics(deleted_uri, vec![], None)
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }

        self.parse_many_and_publish(files_to_reparse).await;
    }

    async fn did_change_workspace_folders(&self, params: DidChangeWorkspaceFoldersParams) {
        self.wait_until_ready().await;
        lifecycle::handle_did_change_workspace_folders(self, params).await
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        self.wait_until_ready().await;
        hover::handle_hover(self, params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        self.wait_until_ready().await;
        goto_definition::handle_goto_definition(self, params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        self.wait_until_ready().await;
        references::handle_references(self, params).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.wait_until_ready().await;
        completion::handle_completion(self, params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        self.wait_until_ready().await;
        code_action::handle_code_action(self, params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        self.wait_until_ready().await;
        rename::prepare_rename(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        self.wait_until_ready().await;
        rename::rename(self, params).await
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
