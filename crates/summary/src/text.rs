pub fn compact_summary_snippet(text: &str, max_chars: usize) -> String {
    let compact = text
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if compact.is_empty() {
        return String::new();
    }
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let mut out = String::new();
    for ch in compact.chars().take(max_chars.saturating_sub(1)) {
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::compact_summary_snippet;

    #[test]
    fn compact_summary_snippet_collapses_whitespace() {
        let compact = compact_summary_snippet("  hello   world   \n  from   test ", 120);
        assert_eq!(compact, "hello world from test");
    }

    #[test]
    fn compact_summary_snippet_truncates_with_ellipsis() {
        let compact = compact_summary_snippet("abcdefghij", 6);
        assert_eq!(compact, "abcde…");
    }
}
