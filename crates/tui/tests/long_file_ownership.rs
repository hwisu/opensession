use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const LINE_LIMIT: usize = 1000;

#[derive(Debug, Deserialize)]
struct OwnershipFile {
    file: Vec<OwnershipEntry>,
}

#[derive(Debug, Deserialize)]
struct OwnershipEntry {
    path: String,
    owner: String,
    domains: Vec<String>,
}

fn walk_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs_files(&path, out);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn oversized_files_require_declared_ownership() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let ownership_path = manifest_dir.join("long_file_ownership.toml");
    let ownership_raw = fs::read_to_string(&ownership_path)
        .expect("read crates/tui/long_file_ownership.toml for ownership policy");
    let ownership: OwnershipFile =
        toml::from_str(&ownership_raw).expect("parse long_file_ownership.toml");

    let mut declared = HashMap::new();
    for entry in ownership.file {
        let key = entry.path.replace('\\', "/");
        assert!(!key.trim().is_empty(), "ownership.path must not be empty");
        assert!(
            !entry.owner.trim().is_empty(),
            "ownership.owner must not be empty for {key}"
        );
        assert!(
            entry.domains.len() >= 2,
            "ownership.domains must contain at least 2 domains for {key}"
        );
        assert!(
            entry.domains.iter().all(|d| !d.trim().is_empty()),
            "ownership.domains must not include empty entries for {key}"
        );
        declared.insert(key, (entry.owner, entry.domains));
    }

    let mut rs_files = Vec::new();
    walk_rs_files(&manifest_dir.join("src"), &mut rs_files);

    let mut oversized = HashSet::new();
    for file in rs_files {
        let content = fs::read_to_string(&file).expect("read rust source");
        let line_count = content.lines().count();
        if line_count > LINE_LIMIT {
            let rel = file
                .strip_prefix(&manifest_dir)
                .expect("strip manifest prefix")
                .to_string_lossy()
                .replace('\\', "/");
            oversized.insert(rel);
        }
    }

    for path in &oversized {
        assert!(
            declared.contains_key(path),
            "Oversized file `{path}` exceeds {LINE_LIMIT} lines and must be declared in long_file_ownership.toml"
        );
    }

    for path in declared.keys() {
        assert!(
            oversized.contains(path),
            "Declared file `{path}` is no longer oversized; remove it from long_file_ownership.toml"
        );
    }
}
