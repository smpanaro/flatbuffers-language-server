use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A graph of the include statement relationships between files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    // TODO: These include transitive dependencies. Should they?
    // Key includes values.
    pub includes: HashMap<PathBuf, Vec<PathBuf>>,
    // Key is included by values.
    pub included_by: HashMap<PathBuf, Vec<PathBuf>>,
}

impl DependencyGraph {
    pub fn update(&mut self, path: &Path, included_paths: Vec<PathBuf>) {
        if let Some(old_included_files) = self.includes.remove(path) {
            for old_included_path in old_included_files {
                if let Some(included_by) = self.included_by.get_mut(&old_included_path) {
                    included_by.retain(|x| x != path);
                }
            }
        }

        for included_path in &included_paths {
            self.included_by
                .entry(included_path.clone())
                .or_default()
                .push(path.to_path_buf());
        }

        self.includes.insert(path.to_path_buf(), included_paths);
    }

    pub fn remove(&mut self, path: &Path) -> Vec<PathBuf> {
        if let Some(included_files) = self.includes.remove(path) {
            for included_path in included_files {
                if let Some(included_by) = self.included_by.get_mut(&included_path) {
                    included_by.retain(|x| x != path);
                }
            }
        }

        if let Some(included_by_files) = self.included_by.remove(path) {
            return included_by_files;
        }

        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_update() {
        let mut graph = DependencyGraph::default();
        let path_a = PathBuf::from("a.fbs");
        let path_b = PathBuf::from("b.fbs");
        let path_c = PathBuf::from("c.fbs");

        graph.update(&path_a, vec![path_b.clone(), path_c.clone()]);

        assert_eq!(
            graph.includes.get(&path_a).unwrap(),
            &vec![path_b.clone(), path_c.clone()]
        );
        assert_eq!(
            graph.included_by.get(&path_b).unwrap(),
            &vec![path_a.clone()]
        );
        assert_eq!(
            graph.included_by.get(&path_c).unwrap(),
            &vec![path_a.clone()]
        );

        graph.update(&path_b, vec![path_c.clone()]);

        assert_eq!(
            graph.includes.get(&path_a).unwrap(),
            &vec![path_b.clone(), path_c.clone()]
        );
        assert_eq!(graph.includes.get(&path_b).unwrap(), &vec![path_c.clone()]);
        assert_eq!(
            graph.included_by.get(&path_b).unwrap(),
            &vec![path_a.clone()]
        );
        assert_eq!(
            graph.included_by.get(&path_c).unwrap(),
            &vec![path_a.clone(), path_b.clone()]
        );
    }

    #[test]
    fn test_update_and_remove() {
        let mut graph = DependencyGraph::default();
        let path_a = PathBuf::from("a.fbs");
        let path_b = PathBuf::from("b.fbs");

        graph.update(&path_a, vec![path_b.clone()]);
        assert_eq!(graph.includes.len(), 1);
        assert_eq!(graph.included_by.len(), 1);

        graph.remove(&path_a);
        assert!(graph.includes.is_empty());
        assert!(graph.included_by.get(&path_b).unwrap().is_empty());
    }
}
