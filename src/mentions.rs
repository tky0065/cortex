use anyhow::{Context, Result};
use base64::Engine as _;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_SUGGESTIONS: usize = 80;
const MAX_SCAN_DEPTH: usize = 6;
const MAX_SCAN_ENTRIES: usize = 2_000;
const MAX_CONTEXT_CHARS: usize = 24_000;
const MAX_FILE_CHARS: usize = 8_000;
const MAX_FOLDER_FILES: usize = 8;
const MAX_TREE_ENTRIES: usize = 120;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathSuggestion {
    pub path: String,
    pub description: String,
    pub is_dir: bool,
}

pub fn path_suggestions(prefix: &str) -> Vec<PathSuggestion> {
    let Ok(root) = std::env::current_dir() else {
        return Vec::new();
    };
    let prefix = prefix.trim_start_matches('@').to_ascii_lowercase();
    let mut suggestions = Vec::new();
    let mut visited = 0;
    collect_path_suggestions(&root, &root, 0, &prefix, &mut visited, &mut suggestions);
    suggestions.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.path
                .to_ascii_lowercase()
                .cmp(&b.path.to_ascii_lowercase())
        })
    });
    suggestions.truncate(MAX_SUGGESTIONS);
    suggestions
}

pub fn resolve_prompt_mentions(prompt: &str) -> String {
    let mentions = extract_path_mentions(prompt);
    if mentions.is_empty() {
        return String::new();
    }

    let Ok(root) = std::env::current_dir() else {
        return String::new();
    };
    let mut remaining = MAX_CONTEXT_CHARS;
    let mut output = String::from("\n\n## Mentioned Project Context\n");
    remaining = remaining.saturating_sub(output.chars().count());

    for mention in mentions {
        if remaining == 0 {
            break;
        }
        let block = match resolve_one(&root, &mention, remaining) {
            Ok(block) => block,
            Err(e) => format!("\n### @{mention}\nWarning: {e}\n"),
        };
        append_bounded(&mut output, &block, &mut remaining);
    }

    if output.trim() == "## Mentioned Project Context" {
        String::new()
    } else {
        output
    }
}

/// Resolves `@paste:{filename}` tokens inserted by the clipboard-paste feature.
///
/// Each token is expanded into a Markdown block containing the PNG as a base64
/// data-URL, so vision-capable LLMs receive the image alongside the prompt text.
/// Tokens that refer to missing or unreadable files are silently skipped.
pub fn resolve_paste_images(prompt: &str) -> String {
    let filenames = extract_paste_tokens(prompt);
    if filenames.is_empty() {
        return String::new();
    }
    let temp_dir = match dirs::home_dir() {
        Some(h) => h.join(".cortex").join("temp"),
        None => return String::new(),
    };
    let mut output = String::from("\n\n## Pasted Images\n");
    for filename in filenames {
        let path = temp_dir.join(&filename);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            _ => "image/png",
        };
        output.push_str(&format!(
            "\n![pasted image](data:{};base64,{})\n",
            mime, b64
        ));
    }
    if output.trim() == "## Pasted Images" {
        String::new()
    } else {
        output
    }
}

fn extract_paste_tokens(prompt: &str) -> Vec<String> {
    let mut tokens = BTreeSet::new();
    for token in prompt.split_whitespace() {
        let token = token.trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ':' | ';' | ')' | ']' | '}' | '"' | '\'')
        });
        if let Some(filename) = token.strip_prefix("@paste:") {
            if !filename.is_empty() && !filename.contains('/') && !filename.contains("..") {
                tokens.insert(filename.to_string());
            }
        }
    }
    tokens.into_iter().collect()
}

fn collect_path_suggestions(
    root: &Path,
    dir: &Path,
    depth: usize,
    prefix: &str,
    visited: &mut usize,
    suggestions: &mut Vec<PathSuggestion>,
) {
    if depth > MAX_SCAN_DEPTH
        || *visited >= MAX_SCAN_ENTRIES
        || suggestions.len() >= MAX_SUGGESTIONS
    {
        return;
    }

    let Ok(mut entries) =
        fs::read_dir(dir).map(|rd| rd.filter_map(|entry| entry.ok()).collect::<Vec<_>>())
    else {
        return;
    };
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        if *visited >= MAX_SCAN_ENTRIES || suggestions.len() >= MAX_SUGGESTIONS {
            break;
        }
        *visited += 1;

        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip(&name, &path) {
            continue;
        }
        let is_dir = path.is_dir();
        let Some(mut rel) = rel_path(root, &path) else {
            continue;
        };
        if rel.chars().any(char::is_whitespace) {
            continue;
        }
        if is_dir {
            rel.push('/');
        }
        let searchable = rel.to_ascii_lowercase();
        if prefix.is_empty()
            || searchable.starts_with(prefix)
            || searchable
                .split('/')
                .any(|segment| segment.starts_with(prefix))
        {
            suggestions.push(PathSuggestion {
                path: rel,
                description: if is_dir { "Directory" } else { "File" }.to_string(),
                is_dir,
            });
        }
        if is_dir {
            collect_path_suggestions(root, &path, depth + 1, prefix, visited, suggestions);
        }
    }
}

fn extract_path_mentions(prompt: &str) -> Vec<String> {
    let mut mentions = BTreeSet::new();
    for token in prompt.split_whitespace() {
        let token = token.trim_matches(|ch: char| {
            matches!(ch, ',' | '.' | ':' | ';' | ')' | ']' | '}' | '"' | '\'')
        });
        let Some(path) = token.strip_prefix('@') else {
            continue;
        };
        if path.is_empty() || path.starts_with("skill:") || path.contains("..") {
            continue;
        }
        mentions.insert(path.trim_end_matches(',').to_string());
    }
    mentions.into_iter().collect()
}

fn resolve_one(root: &Path, mention: &str, remaining: usize) -> Result<String> {
    let clean = mention.trim_start_matches('/').trim_end_matches('/');
    validate_relative(clean)?;
    let path = root.join(clean);
    let canonical_root = root
        .canonicalize()
        .context("failed to resolve project root")?;
    let canonical = path
        .canonicalize()
        .with_context(|| format!("@{mention} was not found"))?;
    if !canonical.starts_with(&canonical_root) {
        anyhow::bail!("@{mention} is outside the project");
    }
    let file_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if should_skip_path(&canonical) || should_skip(file_name, &canonical) {
        anyhow::bail!("@{mention} is ignored");
    }

    if canonical.is_file() {
        return file_block(root, &canonical, remaining);
    }
    if canonical.is_dir() {
        return folder_block(root, &canonical, remaining);
    }
    anyhow::bail!("@{mention} is not a file or directory")
}

fn file_block(root: &Path, path: &Path, remaining: usize) -> Result<String> {
    let rel = rel_path(root, path).unwrap_or_else(|| path.display().to_string());
    let content = read_text_file(path, MAX_FILE_CHARS.min(remaining))?;
    Ok(format!("\n### @{rel}\n```text\n{content}\n```\n"))
}

fn folder_block(root: &Path, dir: &Path, remaining: usize) -> Result<String> {
    let rel = rel_path(root, dir).unwrap_or_else(|| dir.display().to_string());
    let mut tree = Vec::new();
    let mut files = Vec::new();
    collect_folder_context(root, dir, 0, &mut tree, &mut files);

    let mut block = format!("\n### @{rel}/\nTree:\n");
    for entry in tree.into_iter().take(MAX_TREE_ENTRIES) {
        block.push_str("- ");
        block.push_str(&entry);
        block.push('\n');
    }

    let mut left = remaining.saturating_sub(block.chars().count());
    for file in files.into_iter().take(MAX_FOLDER_FILES) {
        if left == 0 {
            break;
        }
        if let Ok(content) = read_text_file(&file, MAX_FILE_CHARS.min(left)) {
            let file_rel = rel_path(root, &file).unwrap_or_else(|| file.display().to_string());
            let snippet = format!("\n#### {file_rel}\n```text\n{content}\n```\n");
            append_bounded(&mut block, &snippet, &mut left);
        }
    }
    Ok(block)
}

fn collect_folder_context(
    root: &Path,
    dir: &Path,
    depth: usize,
    tree: &mut Vec<String>,
    files: &mut Vec<PathBuf>,
) {
    if depth > 3 || tree.len() >= MAX_TREE_ENTRIES {
        return;
    }
    let Ok(mut entries) =
        fs::read_dir(dir).map(|rd| rd.filter_map(|entry| entry.ok()).collect::<Vec<_>>())
    else {
        return;
    };
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if should_skip(&name, &path) {
            continue;
        }
        let Some(mut rel) = rel_path(root, &path) else {
            continue;
        };
        if path.is_dir() {
            rel.push('/');
            tree.push(rel);
            collect_folder_context(root, &path, depth + 1, tree, files);
        } else if path.is_file() {
            tree.push(rel);
            if is_text_candidate(&path) {
                files.push(path);
            }
        }
        if tree.len() >= MAX_TREE_ENTRIES {
            break;
        }
    }
}

fn read_text_file(path: &Path, max_chars: usize) -> Result<String> {
    if !is_text_candidate(path) {
        anyhow::bail!("{} is not a supported text file", path.display());
    }
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(take_chars(&content, max_chars))
}

fn append_bounded(output: &mut String, block: &str, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    let len = block.chars().count();
    if len <= *remaining {
        output.push_str(block);
        *remaining -= len;
    } else {
        output.push_str(&take_chars(block, *remaining));
        *remaining = 0;
    }
}

fn take_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn validate_relative(path: &str) -> Result<()> {
    let rel = Path::new(path);
    if rel.is_absolute() {
        anyhow::bail!("absolute paths are not allowed");
    }
    for component in rel.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            anyhow::bail!("path traversal is not allowed");
        }
    }
    Ok(())
}

fn rel_path(root: &Path, path: &Path) -> Option<String> {
    path.strip_prefix(root)
        .ok()
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
}

fn should_skip_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_str().is_some_and(should_skip_name))
}

fn should_skip(name: &str, path: &Path) -> bool {
    should_skip_name(name) || looks_sensitive(name) || is_binary_path(path)
}

fn should_skip_name(name: &str) -> bool {
    const SKIP_NAMES: &[&str] = &[
        ".git",
        ".agents",
        ".claude",
        ".codex",
        ".cortex",
        ".hg",
        ".svn",
        ".env",
        ".env.local",
        ".env.production",
        ".venv",
        "venv",
        "__pycache__",
        "target",
        "node_modules",
        "dist",
        "build",
        ".next",
        ".nuxt",
        ".svelte-kit",
        ".cache",
        "coverage",
        "vendor",
        "cortex-output",
    ];
    SKIP_NAMES.contains(&name) || (name.ends_with(".lock") && name != "Cargo.lock")
}

fn looks_sensitive(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".p12")
        || lower.ends_with(".pfx")
}

fn is_binary_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "ico"
                    | "pdf"
                    | "zip"
                    | "tar"
                    | "gz"
                    | "7z"
                    | "mp3"
                    | "mp4"
                    | "mov"
                    | "wasm"
                    | "rlib"
                    | "dylib"
                    | "so"
                    | "dll"
                    | "exe"
            )
        })
}

fn is_text_candidate(path: &Path) -> bool {
    path.is_file() && !is_binary_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct CwdGuard(PathBuf);

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    fn temp_project() -> (PathBuf, CwdGuard) {
        let old = std::env::current_dir().unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cortex-mentions-{stamp}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        fs::write(root.join("node_modules/pkg/index.js"), "ignored").unwrap();
        std::env::set_current_dir(&root).unwrap();
        (root, CwdGuard(old))
    }

    #[test]
    fn suggestions_skip_ignored_dirs() {
        let (_root, _guard) = temp_project();
        let suggestions = path_suggestions("src");
        assert!(suggestions.iter().any(|item| item.path == "src/"));
        assert!(suggestions.iter().any(|item| item.path == "src/main.rs"));
        assert!(
            !suggestions
                .iter()
                .any(|item| item.path.contains("node_modules"))
        );
    }

    #[test]
    fn resolves_file_mentions() {
        let (_root, _guard) = temp_project();
        let context = resolve_prompt_mentions("explain @src/main.rs");
        assert!(context.contains("## Mentioned Project Context"));
        assert!(context.contains("fn main()"));
    }

    #[test]
    fn rejects_traversal_mentions() {
        let (_root, _guard) = temp_project();
        let context = resolve_prompt_mentions("read @../secret");
        assert!(!context.contains("secret"));
    }
}
