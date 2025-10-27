use dashmap::DashSet;
use ignore::{WalkBuilder, WalkState};
use log::{debug, error};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::{
    ext::duration::DurationFormat,
    utils::paths::{get_intermediate_paths, is_flatbuffer_schema_path},
};

/// Maintains the workspace file and folder layout.
#[derive(Debug)]
pub struct WorkspaceLayout {
    /// Paths that have a known_file as a descendant.
    pub search_paths: Vec<PathBuf>,
    pub workspace_roots: HashSet<PathBuf>,
    /// Known FlatBuffers schema files.
    known_files: HashSet<PathBuf>,
}

impl WorkspaceLayout {
    pub fn new() -> Self {
        Self {
            search_paths: Vec::new(),
            workspace_roots: HashSet::new(),
            known_files: HashSet::new(),
        }
    }

    pub fn add_root(&mut self, root: PathBuf) -> bool {
        self.workspace_roots.insert(root)
    }

    pub fn add_roots(&mut self, roots: impl IntoIterator<Item = PathBuf>) {
        for root in roots {
            self.add_root(root);
        }
    }

    pub fn remove_root(&mut self, root: &PathBuf) {
        self.workspace_roots.remove(root);
        self.known_files.retain(|f| !f.starts_with(root));
        self.search_paths.retain(|sp| !sp.starts_with(root));
    }

    pub fn discover_files(&mut self) -> Vec<PathBuf> {
        self.known_files.clear();
        self.search_paths.clear();

        let roots: Vec<_> = self.workspace_roots.iter().cloned().collect();
        self.scan_dirs(roots);

        self.known_files.iter().cloned().collect()
    }

    fn scan_dirs(&mut self, paths: Vec<PathBuf>) {
        let start = Instant::now();

        if paths.is_empty() {
            return;
        }

        let mut builder = WalkBuilder::new(&paths[0]);
        if paths.len() > 1 {
            for d in &paths[1..] {
                builder.add(d);
            }
        }

        let new_files = DashSet::new();

        builder.build_parallel().run(|| {
            let new_files = &new_files;
            Box::new(move |result| {
                if let Ok(entry) = result {
                    if is_flatbuffer_schema_path(entry.path()) {
                        if let Ok(path) = fs::canonicalize(entry.path()) {
                            new_files.insert(path.to_path_buf());
                        }
                    }
                }
                WalkState::Continue
            })
        });

        debug!(
            "discovered files in {}: {:?}",
            start.elapsed().log_str(),
            new_files.clone().into_iter().collect::<Vec<PathBuf>>(),
        );

        self.known_files.extend(new_files);
        self.update_search_paths();
    }

    /// Add a new file. Returns true if the file was not already known.
    pub fn add_file(&mut self, path: PathBuf) {
        if is_flatbuffer_schema_path(&path) {
            self.search_paths.extend(self.search_paths_for_path(&path));
            self.known_files.insert(path);
        } else {
            // TODO: Support folders when its needed.
            error!("unexpected file added: {}", path.display());
        }
    }

    pub fn remove_file(&mut self, path: &PathBuf) {
        self.known_files.remove(path);

        // TODO: Good use case for a better data structure.
        // Walking known_files + trying every search path
        // derived from path would be painful.
        self.search_paths.clear();
        self.update_search_paths();
    }

    /// Find the known files that have the provided path as a prefix.
    pub fn known_matching_files(&self, path: &PathBuf) -> Vec<PathBuf> {
        self.known_files
            .iter()
            .filter(|fp| fp.starts_with(path))
            .cloned()
            .collect()
    }

    /// Update search_paths so it contains every directory that is
    /// both an ancestor of a known_file and a descendant of a
    /// workspace_root (include the roots themselves).
    fn update_search_paths(&mut self) {
        let mut new_paths = HashSet::new();
        for f in self.known_files.iter() {
            new_paths.extend(self.search_paths_for_path(f));
        }

        self.search_paths.extend(new_paths);
    }

    fn search_paths_for_path(&self, path: &Path) -> HashSet<PathBuf> {
        get_intermediate_paths(path, &self.workspace_roots)
    }
}
