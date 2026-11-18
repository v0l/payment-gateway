pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(512));
    for c in raw.chars().take(512) {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn newlines_and_escapes_cannot_forge_log_lines() {
        let c = clean("ok\nERROR fake entry\r\u{1b}[31m");
        assert!(!c.contains('\n') && !c.contains('\r') && !c.contains('\u{1b}'));
        assert!(c.starts_with("ok\\nERROR"));
    }

    #[test]
    fn long_input_is_clipped() {
        assert_eq!(clean(&"a".repeat(5000)).len(), 512);
    }
}
