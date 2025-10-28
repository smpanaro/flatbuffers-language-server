use std::{
    collections::HashSet,
    fs::{self, File},
    path::PathBuf,
};

use flatbuffers_language_server::workspace_layout::WorkspaceLayout;
use tempfile::tempdir;

#[test]
fn test_add_roots_and_discover_files() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    let root2 = dir.path().join("root2");
    fs::create_dir_all(&root1).unwrap();
    fs::create_dir_all(&root2).unwrap();

    let subdir = root1.join("subdir");
    fs::create_dir(&subdir).unwrap();

    let fbs_files = vec![
        root1.join("a.fbs"),
        root1.join("b.fbs"),
        subdir.join("c.fbs"),
        root2.join("d.fbs"),
        root2.join("e.fbs"),
    ];
    for f in &fbs_files {
        File::create(f).unwrap();
    }

    let other_files = vec![root1.join("test.txt"), root2.join("other.rs")];
    for f in &other_files {
        File::create(f).unwrap();
    }

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    let canonical_root2 = fs::canonicalize(&root2).unwrap();
    layout.add_roots(vec![canonical_root1.clone(), canonical_root2.clone()]);
    layout.discover_files();

    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    let expected_files: HashSet<PathBuf> = fbs_files
        .into_iter()
        .map(|p| fs::canonicalize(p).unwrap())
        .collect();
    assert_eq!(known_files, expected_files);

    let expected_search_paths: HashSet<PathBuf> = vec![
        fs::canonicalize(root1).unwrap(),
        fs::canonicalize(subdir).unwrap(),
        fs::canonicalize(root2).unwrap(),
    ]
    .into_iter()
    .collect();
    assert_eq!(layout.search_paths, expected_search_paths);
}

#[test]
fn test_remove_roots() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    let root2 = dir.path().join("root2");
    fs::create_dir_all(&root1).unwrap();
    fs::create_dir_all(&root2).unwrap();

    let fbs_files_root1 = vec![root1.join("a.fbs"), root1.join("b.fbs")];
    for f in &fbs_files_root1 {
        File::create(f).unwrap();
    }

    let fbs_files_root2 = vec![root2.join("d.fbs"), root2.join("e.fbs")];
    for f in &fbs_files_root2 {
        File::create(f).unwrap();
    }

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    let canonical_root2 = fs::canonicalize(&root2).unwrap();
    layout.add_roots(vec![canonical_root1.clone(), canonical_root2.clone()]);
    layout.discover_files();

    layout.remove_root(&canonical_root1);

    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    let expected_files: HashSet<PathBuf> = fbs_files_root2
        .into_iter()
        .map(|p| fs::canonicalize(p).unwrap())
        .collect();
    assert_eq!(known_files, expected_files);

    let expected_search_paths: HashSet<PathBuf> =
        vec![fs::canonicalize(root2).unwrap()].into_iter().collect();
    assert_eq!(layout.search_paths, expected_search_paths);
}

#[test]
fn test_add_file() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    fs::create_dir_all(&root1).unwrap();

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    layout.add_root(canonical_root1.clone());

    let new_file_1 = root1.join("new1.fbs");
    File::create(&new_file_1).unwrap();
    let canonical_new_file_1 = fs::canonicalize(&new_file_1).unwrap();
    layout.add_file(canonical_new_file_1.clone());

    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    assert_eq!(
        known_files,
        vec![canonical_new_file_1.clone()].into_iter().collect()
    );
    assert_eq!(
        layout.search_paths,
        vec![canonical_root1.clone()].into_iter().collect()
    );

    let new_file_2 = root1.join("new2.fbs");
    File::create(&new_file_2).unwrap();
    let canonical_new_file_2 = fs::canonicalize(&new_file_2).unwrap();
    layout.add_file(canonical_new_file_2.clone());

    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    assert_eq!(
        known_files,
        vec![canonical_new_file_1, canonical_new_file_2]
            .into_iter()
            .collect()
    );
    assert_eq!(
        layout.search_paths,
        vec![canonical_root1].into_iter().collect()
    );
}

#[test]
fn test_remove_file() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    fs::create_dir_all(&root1).unwrap();

    let file_a = root1.join("a.fbs");
    let file_b = root1.join("b.fbs");
    File::create(&file_a).unwrap();
    File::create(&file_b).unwrap();

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    layout.add_root(canonical_root1.clone());
    layout.discover_files();

    let canonical_file_a = fs::canonicalize(&file_a).unwrap();
    layout.remove_file(&canonical_file_a);

    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    let canonical_file_b = fs::canonicalize(&file_b).unwrap();
    assert_eq!(known_files, vec![canonical_file_b].into_iter().collect());

    assert_eq!(
        layout.search_paths,
        vec![canonical_root1].into_iter().collect()
    );
}

#[test]
fn test_remove_file_last_in_dir() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    let subdir = root1.join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    let file_c = subdir.join("c.fbs");
    File::create(&file_c).unwrap();

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    layout.add_root(canonical_root1.clone());
    layout.discover_files();

    let canonical_file_c = fs::canonicalize(&file_c).unwrap();
    layout.remove_file(&canonical_file_c);

    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    assert!(known_files.is_empty());

    assert!(layout.search_paths.is_empty());
}

#[test]
fn test_add_file_no_duplicate_search_paths() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    fs::create_dir_all(&root1).unwrap();

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    layout.add_root(canonical_root1.clone());

    let new_file_1 = root1.join("new1.fbs");
    File::create(&new_file_1).unwrap();
    let canonical_new_file_1 = fs::canonicalize(&new_file_1).unwrap();
    layout.add_file(canonical_new_file_1.clone());

    let new_file_2 = root1.join("new2.fbs");
    File::create(&new_file_2).unwrap();
    let canonical_new_file_2 = fs::canonicalize(&new_file_2).unwrap();
    layout.add_file(canonical_new_file_2.clone());

    assert_eq!(layout.search_paths.len(), 1);
    assert!(layout.search_paths.contains(&canonical_root1));
}

#[test]
fn test_overlapping_roots() {
    let dir = tempdir().unwrap();
    let root1 = dir.path().join("root1");
    let subdir = root1.join("subdir");
    fs::create_dir_all(&subdir).unwrap();

    let fbs_files = vec![root1.join("a.fbs"), subdir.join("b.fbs")];
    for f in &fbs_files {
        File::create(f).unwrap();
    }

    let mut layout = WorkspaceLayout::new();
    let canonical_root1 = fs::canonicalize(&root1).unwrap();
    let canonical_subdir = fs::canonicalize(&subdir).unwrap();
    layout.add_roots(vec![canonical_root1.clone(), canonical_subdir.clone()]);
    layout.discover_files();

    let expected_files: HashSet<PathBuf> = fbs_files
        .into_iter()
        .map(|p| fs::canonicalize(p).unwrap())
        .collect();
    let canonical_dir = fs::canonicalize(dir.path()).unwrap();
    let known_files: HashSet<PathBuf> = layout
        .known_matching_files(&canonical_dir)
        .into_iter()
        .collect();
    assert_eq!(known_files, expected_files);

    let expected_search_paths: HashSet<PathBuf> = vec![canonical_root1, canonical_subdir]
        .into_iter()
        .collect();
    assert_eq!(layout.search_paths, expected_search_paths);
}
