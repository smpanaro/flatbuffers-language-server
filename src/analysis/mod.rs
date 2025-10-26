pub mod dependency_graph;
pub mod diagnostic_store;
pub mod root_type_store;
pub mod snapshot;
pub mod symbol_index;
pub mod workspace_index;

pub use crate::analysis::snapshot::WorkspaceSnapshot;
use crate::analysis::workspace_index::WorkspaceIndex;
use crate::document_store::DocumentStore;
use crate::ext::duration::DurationFormat;
use crate::parser::Parser;
use crate::search_path_manager::SearchPathManager;
use crate::symbol_table::RootTypeInfo;
use crate::utils::paths::{
    get_intermediate_paths, is_flatbuffer_schema, is_flatbuffer_schema_path, path_buf_to_url,
    uri_to_path_buf,
};
use dashmap::DashSet;
use ignore::{WalkBuilder, WalkState};
use log::{debug, info};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tower_lsp_server::lsp_types::{Diagnostic, FileChangeType, Uri};
use tower_lsp_server::UriExt;

#[derive(Debug)]
pub struct AnalysisEngine {
    index: RwLock<WorkspaceIndex>,
    documents: Arc<DocumentStore>,
    search_paths: Arc<SearchPathManager>,
}

impl AnalysisEngine {
    fn update_symbols(
        &self,
        index: &mut WorkspaceIndex,
        path: &PathBuf,
        st: crate::symbol_table::SymbolTable,
        included_files: Vec<PathBuf>,
        root_type_info: Option<RootTypeInfo>,
    ) {
        if let Some(old_symbol_keys) = index.symbols.per_file.remove(path) {
            for key in old_symbol_keys {
                index.symbols.global.remove(&key);
            }
        }
        index.root_types.root_types.remove(path);

        self.update_includes(index, path, included_files);

        let symbol_map = st.into_inner();
        let new_symbol_keys: Vec<String> = symbol_map.keys().cloned().collect();
        for (key, symbol) in symbol_map {
            index.symbols.global.insert(key, symbol);
        }
        index.symbols.per_file.insert(path.clone(), new_symbol_keys);

        if let Some(rti) = root_type_info {
            index.root_types.root_types.insert(path.clone(), rti);
        }
    }

    fn update_includes(
        &self,
        index: &mut WorkspaceIndex,
        path: &PathBuf,
        included_paths: Vec<PathBuf>,
    ) {
        if let Some(old_included_files) = index.dependencies.includes.remove(path) {
            for old_included_path in old_included_files {
                if let Some(included_by) =
                    index.dependencies.included_by.get_mut(&old_included_path)
                {
                    included_by.retain(|x| x != path);
                }
            }
        }

        for included_path in &included_paths {
            index
                .dependencies
                .included_by
                .entry(included_path.clone())
                .or_default()
                .push(path.clone());
        }

        index
            .dependencies
            .includes
            .insert(path.clone(), included_paths);
    }

    pub fn new(documents: Arc<DocumentStore>, search_paths: Arc<SearchPathManager>) -> Self {
        Self {
            index: RwLock::new(WorkspaceIndex::new()),
            documents,
            search_paths,
        }
    }

    pub async fn snapshot<'a>(&'a self) -> WorkspaceSnapshot<'a> {
        WorkspaceSnapshot {
            index: self.index.read().await,
            documents: Arc::new(self.documents.document_map.clone()),
        }
    }

    pub async fn update_search_paths_and_discover_files(&self) -> Vec<PathBuf> {
        let start = Instant::now();
        let roots: Vec<_> = self
            .search_paths
            .workspace_roots
            .iter()
            .map(|r| r.key().clone())
            .collect();

        if roots.is_empty() {
            let mut search_paths_guard = self.search_paths.search_paths.write().await;
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

        let mut search_paths_guard = self.search_paths.search_paths.write().await;
        *search_paths_guard = search_paths;

        fbs_files.into_iter().collect::<Vec<_>>()
    }

    pub async fn scan_workspace(&self) -> Vec<(Uri, Vec<Diagnostic>)> {
        let index = self.index.read().await;
        let fbs_files = self
            .update_search_paths_and_discover_files()
            .await
            .into_iter()
            .filter(|uri| !index.symbols.per_file.contains_key(uri))
            .collect::<Vec<_>>();
        drop(index);
        self.parse_many_and_publish(fbs_files).await
    }

    pub async fn remove_workspace_folder(&self, folder_uri: &Uri) -> Vec<(Uri, Vec<Diagnostic>)> {
        let Ok(folder_path) = uri_to_path_buf(folder_uri) else {
            return vec![];
        };

        let mut diagnostics_to_publish = Vec::new();
        let mut files_to_reparse;

        {
            let mut index = self.index.write().await;
            let uris_to_remove: HashSet<PathBuf> = index
                .symbols
                .per_file
                .keys()
                .filter(|path| path.starts_with(&folder_path))
                .cloned()
                .collect();

            files_to_reparse = HashSet::new();
            for path in &uris_to_remove {
                let affected_files = index.remove_file(path);
                for f in affected_files {
                    if !uris_to_remove.contains(&f) {
                        files_to_reparse.insert(f);
                    }
                }

                if let Ok(uri) = path_buf_to_url(path) {
                    diagnostics_to_publish.push((uri, vec![]));
                }
            }

            let mut search_paths_guard = self.search_paths.search_paths.write().await;
            search_paths_guard.retain(|path| !path.starts_with(&folder_path));
        }

        let mut reparse_diags = self.parse_many_and_publish(files_to_reparse).await;
        diagnostics_to_publish.append(&mut reparse_diags);
        diagnostics_to_publish
    }

    pub async fn parse_many_and_publish(
        &self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut parsed_in_scan = HashSet::new();
        let mut all_diagnostics = Vec::new();
        for path in paths {
            if !parsed_in_scan.contains(&path) {
                let mut diags = self.parse_and_publish(&path, &mut parsed_in_scan).await;
                all_diagnostics.append(&mut diags);
            }
        }
        all_diagnostics
    }

    async fn parse_and_publish(
        &self,
        path: &PathBuf,
        parsed_files: &mut HashSet<PathBuf>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let search_paths_guard = self.search_paths.search_paths.read().await;
        let mut index = self.index.write().await;

        let mut files_to_parse = vec![path.clone()];
        let mut newly_parsed_files = HashSet::new();
        let mut all_diagnostics = std::collections::HashMap::new();

        while let Some(path) = files_to_parse.pop() {
            if !parsed_files.insert(path.clone()) {
                continue;
            }
            newly_parsed_files.insert(path.clone());

            let content = if let Some(doc) = self.documents.document_map.get(&path) {
                doc.value().to_string()
            } else {
                match tokio::fs::read_to_string(&path).await {
                    Ok(text) => {
                        self.documents
                            .document_map
                            .insert(path.clone(), ropey::Rope::from_str(&text));
                        text
                    }
                    Err(e) => {
                        log::error!("failed to read file {}: {}", path.display(), e);
                        continue;
                    }
                }
            };

            log::info!("parsing: {}", path.display());
            let (diagnostics_map, symbol_table, included_files, root_type_info) =
                crate::parser::FlatcFFIParser.parse(&path, &content, &search_paths_guard);

            if let Some(st) = symbol_table {
                self.update_symbols(
                    &mut index,
                    &path,
                    st,
                    included_files.clone(),
                    root_type_info,
                );
            } else {
                // A parse error occurred, but we don't want to clear the old symbol table
                // as it may be useful to the user while they are editing.
                // We do want to make sure that we are tracking this file's existence,
                // in case it needs to be cleaned up later.
                if !index.symbols.per_file.contains_key(&path) {
                    index.symbols.per_file.insert(path.clone(), vec![]);
                }
                self.update_includes(&mut index, &path, included_files.clone());
            }

            for (file_path, diagnostics) in diagnostics_map {
                all_diagnostics.insert(file_path, diagnostics);
            }

            for included_path in included_files {
                if !parsed_files.contains(&included_path) {
                    files_to_parse.push(included_path);
                }
            }
        }

        let mut files_to_update = HashSet::new();
        files_to_update.insert(path.clone());
        files_to_update.extend(newly_parsed_files);

        let mut diagnostics_to_publish = Vec::new();
        for file_path in files_to_update {
            let mut new_diags = all_diagnostics.get(&file_path).cloned().unwrap_or_default();

            new_diags.sort_by(|a, b| {
                a.message
                    .cmp(&b.message)
                    .then_with(|| a.range.start.cmp(&b.range.start))
            });

            let old_diags = index.diagnostics.published.get(&file_path);

            let has_changed = old_diags.map_or(true, |d| *d != new_diags);
            if !has_changed {
                continue;
            }

            if let Ok(file_uri) = path_buf_to_url(&file_path) {
                diagnostics_to_publish.push((file_uri, new_diags.clone()));
                index.diagnostics.published.insert(file_path, new_diags);
            }
        }

        diagnostics_to_publish
    }

    pub async fn handle_file_changes(
        &self,
        changes: Vec<tower_lsp_server::lsp_types::FileEvent>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut files_to_reparse = HashSet::new();
        let mut diagnostics_to_publish = Vec::new();

        // What about folders? Watching files is sufficient.
        // New folder created     : empty so can't have .fbs files.
        // Existing folder deleted: if it has .fbs they will show up as deleted.
        // Existing folder renamed: if it has .fbs they will show up as deleted
        //                          and created in the new location.
        // ... except from VSCode. For which we handle folders below.

        {
            let mut index = self.index.write().await;

            for event in changes {
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
                    FileChangeType::CREATED => {
                        files_to_reparse.insert(path);
                    }
                    FileChangeType::CHANGED => {
                        // NOTE: This doubles the work done on save,
                        // but allows us to capture file changes made
                        // outside of the client (e.g. git checkout).
                        files_to_reparse.insert(path);
                    }
                    FileChangeType::DELETED => {
                        // VSCode doesn't report the files in a deleted folder, so we do our best.
                        let deleted_files = index.expand_to_known_files(&path);
                        for deleted in deleted_files.clone() {
                            let affected_files = index.remove_file(&deleted);
                            for uri in affected_files {
                                if !deleted_files.contains(&uri) {
                                    files_to_reparse.insert(uri);
                                }
                            }
                            info!("marking {} deleted", deleted.display());
                            self.documents.document_map.remove(&deleted);
                            if let Ok(deleted_uri) = path_buf_to_url(&deleted) {
                                diagnostics_to_publish.push((deleted_uri, vec![]));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut reparse_diags = self.parse_many_and_publish(files_to_reparse).await;
        diagnostics_to_publish.append(&mut reparse_diags);
        diagnostics_to_publish
    }
}
