//! Text put into XML, SVG or HTML: the one escaper every writer of a
//! report, figure or export uses (#1649), so a name such as
//! "watch & test.wav" can't break the document it is written into.

/// `s` escaped for XML text or a double-quoted attribute: `&`, `<`, `>`
/// and `"` as entities, and control characters XML 1.0 can't hold left
/// out (tab, line feed and carriage return are kept).
pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_escaped_and_control_characters_left_out() {
        assert_eq!(
            escape("watch & <test> \"1\".wav"),
            "watch &amp; &lt;test&gt; &quot;1&quot;.wav"
        );
        assert_eq!(escape("a\u{1}b\tc\nd"), "ab\tc\nd");
        assert_eq!(escape("Kew–Teddington 90°"), "Kew–Teddington 90°");
    }
}
