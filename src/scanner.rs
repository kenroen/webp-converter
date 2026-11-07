//! File and folder discovery and output path collision resolution.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::config::ExistingPolicy;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFile {
    pub source_path: PathBuf,
    pub base_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedTarget {
    Ready(PathBuf),
    SkipExisting(PathBuf),
}

pub fn is_webp_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("webp"))
}

fn normalized_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn scan_inputs(inputs: &[PathBuf], recursive: bool) -> Vec<DiscoveredFile> {
    let mut seen = HashSet::new();
    let mut results = Vec::new();

    for input in inputs {
        if input.is_dir() {
            let mut walker = WalkDir::new(input);
            if !recursive {
                walker = walker.max_depth(1);
            }
            for entry in walker.into_iter().filter_map(Result::ok) {
                let path = entry.path();
                if is_webp_file(path) && seen.insert(normalized_key(path)) {
                    results.push(DiscoveredFile {
                        source_path: path.to_path_buf(),
                        base_dir: Some(input.clone()),
                    });
                }
            }
        } else if is_webp_file(input) && seen.insert(normalized_key(input)) {
            results.push(DiscoveredFile {
                source_path: input.clone(),
                base_dir: input.parent().map(Path::to_path_buf),
            });
        }
    }

    results
}

pub fn resolve_output_dir(item: &DiscoveredFile, custom_output_dir: Option<&Path>) -> PathBuf {
    let source_parent = item
        .source_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    match custom_output_dir {
        None => source_parent.to_path_buf(),
        Some(out_root) => {
            if let Some(base) = &item.base_dir
                && let Ok(rel) = source_parent.strip_prefix(base)
                && !rel.as_os_str().is_empty()
            {
                return out_root.join(rel);
            }
            out_root.to_path_buf()
        }
    }
}

pub fn resolve_target_path(
    target_dir: &Path,
    stem: &str,
    ext: &str,
    policy: ExistingPolicy,
) -> ResolvedTarget {
    let candidate = target_dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return ResolvedTarget::Ready(candidate);
    }

    match policy {
        ExistingPolicy::Skip => ResolvedTarget::SkipExisting(candidate),
        ExistingPolicy::Overwrite => ResolvedTarget::Ready(candidate),
        ExistingPolicy::Rename => {
            let mut counter = 1usize;
            loop {
                let next = target_dir.join(format!("{stem} ({counter}).{ext}"));
                if !next.exists() {
                    return ResolvedTarget::Ready(next);
                }
                counter += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_inputs_recursive_vs_non_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let a = root.join("a.webp");
        let b = sub.join("b.WEBP");
        let ignore_png = sub.join("ignore.png");
        let ignore_tmp = root.join("temp.webp-tmp");
        std::fs::write(&a, b"webp").unwrap();
        std::fs::write(&b, b"webp").unwrap();
        std::fs::write(&ignore_png, b"png").unwrap();
        std::fs::write(&ignore_tmp, b"tmp").unwrap();

        let flat = scan_inputs(&[root.to_path_buf()], false);
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].source_path, a);

        let rec = scan_inputs(&[root.to_path_buf()], true);
        assert_eq!(rec.len(), 2);
        assert!(rec.iter().any(|f| f.source_path == a));
        assert!(rec.iter().any(|f| f.source_path == b));
    }

    #[test]
    fn scan_inputs_deduplicates_same_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a = root.join("a.webp");
        std::fs::write(&a, b"webp").unwrap();

        let found = scan_inputs(&[root.to_path_buf(), a.clone()], true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_path, a);
        assert_eq!(found[0].base_dir, Some(root.to_path_buf()));
    }

    #[test]
    fn resolve_output_dir_preserves_relative_structure() {
        let root = PathBuf::from("D:/photos/batch");
        let source = PathBuf::from("D:/photos/batch/sub/deep/item.webp");
        let item = DiscoveredFile {
            source_path: source,
            base_dir: Some(root),
        };
        let out_root = PathBuf::from("D:/converted");
        assert_eq!(
            resolve_output_dir(&item, Some(&out_root)),
            PathBuf::from("D:/converted/sub/deep")
        );
        assert_eq!(
            resolve_output_dir(&item, None),
            PathBuf::from("D:/photos/batch/sub/deep")
        );
    }

    #[test]
    fn resolve_target_path_policies() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let existing = dir.join("pic.png");
        std::fs::write(&existing, b"existing").unwrap();

        assert_eq!(
            resolve_target_path(dir, "pic", "png", ExistingPolicy::Skip),
            ResolvedTarget::SkipExisting(existing.clone())
        );
        assert_eq!(
            resolve_target_path(dir, "pic", "png", ExistingPolicy::Overwrite),
            ResolvedTarget::Ready(existing)
        );

        let renamed_1 = dir.join("pic (1).png");
        assert_eq!(
            resolve_target_path(dir, "pic", "png", ExistingPolicy::Rename),
            ResolvedTarget::Ready(renamed_1.clone())
        );

        std::fs::write(&renamed_1, b"existing 1").unwrap();
        assert_eq!(
            resolve_target_path(dir, "pic", "png", ExistingPolicy::Rename),
            ResolvedTarget::Ready(dir.join("pic (2).png"))
        );
    }
}
