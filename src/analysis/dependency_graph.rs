use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct DependencyGraph {
    pub includes: HashMap<PathBuf, Vec<PathBuf>>,
    pub included_by: HashMap<PathBuf, Vec<PathBuf>>,
}
