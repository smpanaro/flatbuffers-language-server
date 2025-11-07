pub mod dependency_graph;
pub mod diagnostic_store;
pub mod root_type_store;
pub mod snapshot;
pub mod symbol_index;
pub mod workspace_index;

pub use crate::analysis::snapshot::WorkspaceSnapshot;
use crate::analysis::workspace_index::WorkspaceIndex;
use crate::document_store::DocumentStore;
use crate::parser::Parser;
use crate::utils::paths::{is_flatbuffer_schema, path_buf_to_uri, uri_to_path_buf};
use crate::workspace_layout::WorkspaceLayout;
use log::info;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp_server::lsp_types::{Diagnostic, FileChangeType, FileEvent, Uri};
use tower_lsp_server::UriExt;

/// A semantic analyzer for a workspace.
#[derive(Debug)]
pub struct Analyzer {
    index: RwLock<WorkspaceIndex>,
    documents: Arc<DocumentStore>,
    pub layout: RwLock<WorkspaceLayout>,
}

impl Analyzer {
    #[must_use] pub fn new(documents: Arc<DocumentStore>) -> Self {
        Self {
            index: RwLock::new(WorkspaceIndex::new()),
            documents,
            layout: RwLock::new(WorkspaceLayout::new()),
        }
    }

    pub async fn snapshot<'a>(&'a self) -> WorkspaceSnapshot<'a> {
        WorkspaceSnapshot {
            index: self.index.read().await,
            documents: Arc::new(self.documents.document_map.clone()),
        }
    }

    pub async fn handle_workspace_folder_changes(
        &self,
        added: Vec<Uri>,
        removed: Vec<Uri>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut diagnostics: HashMap<Uri, Vec<Diagnostic>> = HashMap::new();

        let added_paths = added
            .iter()
            .filter_map(|u| uri_to_path_buf(u).ok())
            .collect::<Vec<_>>();
        for addition in &added_paths {
            self.add_workspace_folder(addition.clone()).await;
            info!("added root folder: {}", addition.display());
        }

        let mut to_parse = HashSet::new();
        for removed_dir in removed {
            if let Ok(dir_path) = uri_to_path_buf(&removed_dir) {
                let result = self.remove_workspace_folder(&dir_path).await;
                diagnostics.extend(result.diagnostics().into_iter());
                to_parse.extend(result.affected);
                info!("removed root folder: {}", dir_path.display());
            }
        }

        // Handling additions is simpler as a full reparse, so we
        // do that. This also means we can ignore the more targeted
        // removal parse list in this case.
        if !added_paths.is_empty() {
            let mut layout = self.layout.write().await;
            to_parse = layout.discover_files().into_iter().collect();
        }

        diagnostics.extend(self.parse(to_parse).await.into_iter());
        diagnostics.into_iter().collect()
    }

    async fn add_workspace_folder(&self, folder: PathBuf) {
        let mut layout = self.layout.write().await;
        layout.add_root(folder);
    }

    /// Remove the given workspace folder and return affected files.
    async fn remove_workspace_folder(&self, folder: &PathBuf) -> FolderRemoval {
        let mut diagnostics_to_publish: HashMap<Uri, Vec<Diagnostic>> = HashMap::new();
        let mut files_to_reparse;

        let mut layout = self.layout.write().await;
        let mut index = self.index.write().await;
        let to_remove = layout.known_matching_files(folder);

        files_to_reparse = HashSet::new();
        for path in &to_remove {
            let affected_files = index
                .remove(path)
                .into_iter()
                .filter(|p| !to_remove.contains(p))
                .collect::<Vec<_>>();
            files_to_reparse.extend(affected_files);

            if let Ok(uri) = path_buf_to_uri(path) {
                diagnostics_to_publish.entry(uri).or_default();
            }
        }

        layout.remove_root(folder);
        index.diagnostics.remove_dir(folder);

        FolderRemoval {
            removed: to_remove,
            affected: files_to_reparse.into_iter().collect(),
        }
    }

    /// Parse a set of files and return the set of new diagnostics
    /// to publish as a result.
    pub async fn parse(
        &self,
        paths: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut parsed_in_scan = HashSet::new();
        let mut all_diagnostics = Vec::new();
        for path in paths {
            if !parsed_in_scan.contains(&path) {
                let mut diags = self.parse_single(&path, &mut parsed_in_scan).await;
                all_diagnostics.append(&mut diags);
            }
        }
        all_diagnostics
    }

    async fn parse_single(
        &self,
        path: &PathBuf,
        parsed_files: &mut HashSet<PathBuf>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let layout = self.layout.read().await;
        let mut index = self.index.write().await;
        let search_paths: Vec<_> = layout.search_paths.iter().map(PathBuf::from).collect();

        let mut files_to_parse = vec![path.clone()];
        let mut newly_parsed_files = HashSet::new();

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
            let result = crate::parser::FlatcFFIParser.parse(&path, &content, &search_paths);

            for included_path in &result.includes {
                if !parsed_files.contains(included_path) {
                    files_to_parse.push(included_path.clone());
                }
            }

            index.update(&path, result);
        }

        index
            .diagnostics
            .mark_published()
            .into_iter()
            .filter_map(|(k, v)| path_buf_to_uri(&k).ok().map(|u| (u, v)))
            .collect()
    }

    pub async fn handle_file_changes(
        &self,
        changes: Vec<FileEvent>,
    ) -> Vec<(Uri, Vec<Diagnostic>)> {
        let mut files_to_reparse = HashSet::new();
        let mut diagnostics_to_publish: HashMap<Uri, Vec<Diagnostic>> = HashMap::new();

        // What about folders? Watching files is sufficient.
        // New folder created     : empty so can't have .fbs files.
        // Existing folder deleted: if it has .fbs they will show up as deleted.
        // Existing folder renamed: if it has .fbs they will show up as deleted
        //                          and created in the new location.
        // ... except from VSCode. For which we handle folders below.

        {
            let mut layout = self.layout.write().await;
            let mut index = self.index.write().await;

            for event in changes {
                // Canonicalize will fail for deleted files, so fall back to non-canonical.
                // TODO: Figure out a better heuristic (resolve parents or store client<>canonical map or scan).
                let non_canonical = event.uri.to_file_path().map(|p| p.to_path_buf());
                let Some(path) = uri_to_path_buf(&event.uri).ok().or(non_canonical) else {
                    continue;
                };

                let has_ext = path.extension().is_some();
                if !is_flatbuffer_schema(&event.uri) && has_ext {
                    continue;
                }

                match event.typ {
                    FileChangeType::CREATED => {
                        files_to_reparse.insert(path.clone());
                        layout.add_file(path);
                    }
                    FileChangeType::CHANGED => {
                        // NOTE: This doubles the work done on save,
                        // but allows us to capture file changes made
                        // outside of the client (e.g. git checkout).
                        files_to_reparse.insert(path);
                    }
                    FileChangeType::DELETED => {
                        // VSCode doesn't report the files in a deleted folder, so we do our best.
                        let deleted_files = layout.known_matching_files(&path);
                        for deleted in &deleted_files {
                            let affected_files = index
                                .remove(deleted)
                                .into_iter()
                                .filter(|p| !deleted_files.contains(p))
                                .collect::<Vec<_>>();
                            files_to_reparse.extend(affected_files);

                            info!("marking {} deleted", deleted.display());
                            self.documents.document_map.remove(deleted);
                            layout.remove_file(deleted);
                            if let Ok(deleted_uri) = path_buf_to_uri(deleted) {
                                diagnostics_to_publish.entry(deleted_uri).or_default();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let reparse_diags = self.parse(files_to_reparse).await;
        diagnostics_to_publish.extend(reparse_diags);
        diagnostics_to_publish.into_iter().collect()
    }
}

/// The results of removing a folder.
struct FolderRemoval {
    // The removed files.
    removed: Vec<PathBuf>,
    // The files indirectly affected by the removal.
    affected: Vec<PathBuf>,
}

impl FolderRemoval {
    /// Generate the empty diagnostics that should be emitted
    /// to clear any previous diagnostics for removed files.
    // TODO: Maybe this logic can move into DiagnosticStore
    //       (Based on known_files?)
    fn diagnostics(&self) -> HashMap<Uri, Vec<Diagnostic>> {
        self.removed
            .iter()
            .filter_map(|p| path_buf_to_uri(p).ok())
            .map(|u| (u, vec![]))
            .collect()
    }
}
