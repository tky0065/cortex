#![allow(dead_code)]

use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub struct FileSystem {
    root: PathBuf,
}

impl FileSystem {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn read(&self, rel: impl AsRef<Path>) -> Result<String> {
        let path = self.resolve(rel)?;
        fs::read_to_string(&path).with_context(|| format!("read failed: {}", path.display()))
    }

    pub fn write(&self, rel: impl AsRef<Path>, content: &str) -> Result<()> {
        let path = self.resolve(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content).with_context(|| format!("write failed: {}", path.display()))
    }

    pub fn list(&self, rel: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
        let path = self.resolve(rel)?;
        let entries = fs::read_dir(&path)?
            .filter_map(|e| e.ok())
            .map(|e| {
                e.path()
                    .strip_prefix(&self.root)
                    .unwrap_or(&e.path())
                    .to_path_buf()
            })
            .collect();
        Ok(entries)
    }

    fn resolve(&self, rel: impl AsRef<Path>) -> Result<PathBuf> {
        let rel = rel.as_ref();
        // Guard 1: reject traversal + absolute paths in the raw input
        for component in rel.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    bail!("path traversal rejected: {}", rel.display());
                }
                _ => {}
            }
        }
        let abs = normalize_path(&self.root.join(rel));
        // Guard 2: post-normalization containment check
        if !abs.starts_with(&self.root) {
            bail!("path escapes sandbox: {}", abs.display());
        }
        if self.root.exists() {
            let canonical_root = self.root.canonicalize().with_context(|| {
                format!("canonicalize sandbox root failed: {}", self.root.display())
            })?;
            let existing = nearest_existing_path(&abs);
            let canonical_existing = existing
                .canonicalize()
                .with_context(|| format!("canonicalize path failed: {}", existing.display()))?;
            if !canonical_existing.starts_with(&canonical_root) {
                bail!("path escapes sandbox: {}", canonical_existing.display());
            }
        }
        Ok(abs)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn nearest_existing_path(path: &Path) -> PathBuf {
    let mut current = path;
    while !current.exists() {
        if let Some(parent) = current.parent() {
            current = parent;
        } else {
            break;
        }
    }
    current.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        let dir = std::env::temp_dir().join("cortex_fs_test");
        fs::create_dir_all(&dir).unwrap();
        let sandbox = FileSystem::new(&dir);
        assert!(sandbox.read("../etc/passwd").is_err());
        assert!(sandbox.read("/etc/passwd").is_err());
    }

    #[test]
    fn read_write_roundtrip() {
        let dir = std::env::temp_dir().join("cortex_fs_test2");
        fs::create_dir_all(&dir).unwrap();
        let sandbox = FileSystem::new(&dir);
        sandbox.write("test.txt", "hello").unwrap();
        assert_eq!(sandbox.read("test.txt").unwrap(), "hello");
    }

    #[test]
    fn creates_parent_dirs() {
        let dir = std::env::temp_dir().join("cortex_fs_test3");
        fs::create_dir_all(&dir).unwrap();
        let sandbox = FileSystem::new(&dir);
        sandbox.write("a/b/c.txt", "deep").unwrap();
        assert_eq!(sandbox.read("a/b/c.txt").unwrap(), "deep");
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("cortex_fs_symlink_root_{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("cortex_fs_symlink_outside_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();
        symlink(&outside, root.join("escape")).unwrap();

        let sandbox = FileSystem::new(&root);
        assert!(sandbox.read("escape/secret.txt").is_err());
        assert!(sandbox.write("escape/new.txt", "secret").is_err());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
