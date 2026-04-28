#![allow(dead_code)]

/// Compresses `text` so the result is at most `max_chars` long.
/// Strategy: keep the first `head_ratio` fraction and the last `tail_ratio` fraction,
/// inserting a truncation marker in between.
pub fn compress(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_owned();
    }
    if max_chars < 64 {
        // Too small to be useful — return empty rather than garbage
        return String::new();
    }

    let marker = "\n\n[...truncated...]\n\n";
    let available = max_chars.saturating_sub(marker.len());
    let head = available * 2 / 3;
    let tail = available - head;

    let head_str = safe_slice(text, 0, head);
    let tail_start = text.len().saturating_sub(tail);
    let tail_str = safe_slice(text, tail_start, text.len());

    format!("{}{}{}", head_str, marker, tail_str)
}

/// Returns `&text[start..end]` snapped to valid UTF-8 char boundaries.
fn safe_slice(text: &str, start: usize, end: usize) -> &str {
    let start = snap_to_char_boundary(text, start);
    let end   = snap_to_char_boundary(text, end.min(text.len()));
    &text[start..end]
}

fn snap_to_char_boundary(text: &str, mut idx: usize) -> usize {
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_unchanged() {
        let t = "hello world";
        assert_eq!(compress(t, 100), t);
    }

    #[test]
    fn long_text_compressed() {
        let t = "a".repeat(1000);
        let out = compress(&t, 200);
        assert!(out.len() <= 200 + 30); // marker adds ~30 chars
        assert!(out.contains("[...truncated...]"));
    }

    #[test]
    fn too_small_budget_returns_empty() {
        assert_eq!(compress("hello world", 10), "");
    }

    #[test]
    fn utf8_safe() {
        // Chinese characters — 3 bytes each
        let t = "中文测试文本".repeat(200);
        let out = compress(&t, 100);
        // Must be valid UTF-8 (no panic)
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
