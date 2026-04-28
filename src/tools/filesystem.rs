#![allow(dead_code)]

use anyhow::{bail, Context, Result};
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
        fs::read_to_string(&path)
            .with_context(|| format!("read failed: {}", path.display()))
    }

    pub fn write(&self, rel: impl AsRef<Path>, content: &str) -> Result<()> {
        let path = self.resolve(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, content)
            .with_context(|| format!("write failed: {}", path.display()))
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
}
