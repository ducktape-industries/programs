//! A small embedded highlighter: one hand-written scanner over a per-language table (comments,
//! strings, keywords), line by line, carrying only whether a block comment is still open. No
//! grammar, no dependency: colour is a reading aid, not a parser.
use std::ops::Range;
use view::prelude::*;
use view::{FontStyle, FontWeight, HighlightStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Keyword,
    Type,
    String,
    Number,
    Comment,
    // a key: TOML/YAML `key =`/`key:`, a JSON object key
    Property,
}

struct Lang {
    line_comment: &'static [&'static str],
    block_comment: Option<(&'static str, &'static str)>,
    keywords: &'static [&'static str],
    // `key =` / `key:` at the start of a line is a property
    keyed: bool,
}

static RUST: Lang = Lang {
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    keywords: &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ],
    keyed: false,
};

static JS: Lang = Lang {
    line_comment: &["//"],
    block_comment: Some(("/*", "*/")),
    keywords: &[
        "async",
        "await",
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "default",
        "delete",
        "else",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "from",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "interface",
        "let",
        "new",
        "null",
        "of",
        "return",
        "static",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "type",
        "typeof",
        "undefined",
        "var",
        "void",
        "while",
        "yield",
    ],
    keyed: false,
};

static SHELL: Lang = Lang {
    line_comment: &["#"],
    block_comment: None,
    keywords: &[
        "case", "do", "done", "echo", "elif", "else", "esac", "exit", "export", "fi", "for",
        "function", "if", "in", "local", "return", "set", "then", "while",
    ],
    keyed: false,
};

static CONFIG: Lang = Lang {
    line_comment: &["#"],
    block_comment: None,
    keywords: &["true", "false", "null", "yes", "no", "on", "off"],
    keyed: true,
};

static JSON: Lang = Lang {
    line_comment: &[],
    block_comment: None,
    keywords: &["true", "false", "null"],
    keyed: false,
};

// The table for a file name, by extension; `None` is plain text.
fn lang(name: &str) -> Option<&'static Lang> {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    Some(match ext {
        "rs" | "rust" => &RUST,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "javascript" | "typescript" => &JS,
        "sh" | "bash" | "zsh" | "shell" => &SHELL,
        "toml" | "yaml" | "yml" => &CONFIG,
        "json" => &JSON,
        _ => return None,
    })
}

// The language a file's name says it is written in, as the phone's file
// bar names it (the Expo app's `highlight.ts` `languageOf`): markdown,
// else named from the table [`lang`] picks; `None` for plain text.
pub fn language_of(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".md") || lower.ends_with(".markdown") {
        return Some("markdown");
    }
    let table = lang(name)?;
    let is = |other: &Lang| std::ptr::eq(table, other);
    Some(if is(&RUST) {
        "rust"
    } else if is(&JS) {
        if lower.ends_with(".ts") || lower.ends_with(".tsx") {
            "typescript"
        } else {
            "javascript"
        }
    } else if is(&SHELL) {
        "shell"
    } else if is(&CONFIG) {
        if lower.ends_with(".toml") {
            "toml"
        } else {
            "yaml"
        }
    } else {
        "json"
    })
}

// Tokens of every line of `text`, by line, for a file `name` or a fenced
// block's language. Markdown source is its own case: a heading or a fence
// line is one token, the rest is prose.
pub fn tokens(name: &str, lines: &[&str]) -> Vec<Vec<(Range<usize>, Token)>> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".md") || lower == "md" || lower == "markdown" {
        return lines
            .iter()
            .map(|line| {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') {
                    vec![(0..line.len(), Token::Keyword)]
                } else if trimmed.starts_with("```") || trimmed.starts_with('>') {
                    vec![(0..line.len(), Token::Comment)]
                } else {
                    Vec::new()
                }
            })
            .collect();
    }
    let Some(lang) = lang(name) else {
        return vec![Vec::new(); lines.len()];
    };
    let mut open = false;
    lines
        .iter()
        .map(|line| scan(lang, line, &mut open))
        .collect()
}

fn scan(lang: &Lang, line: &str, open: &mut bool) -> Vec<(Range<usize>, Token)> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    if *open {
        let (_, close) = lang.block_comment.expect("only a block comment stays open");
        match line.find(close) {
            Some(end) => {
                at = end + close.len();
                *open = false;
            }
            None => at = line.len(),
        }
        out.push((0..at, Token::Comment));
    }
    let first_word = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(0);
    while at < bytes.len() {
        let rest = &line[at..];
        let byte = bytes[at];
        if lang
            .line_comment
            .iter()
            .any(|marker| rest.starts_with(marker))
        {
            out.push((at..line.len(), Token::Comment));
            break;
        }
        if let Some((start, close)) = lang.block_comment
            && rest.starts_with(start)
        {
            let end = rest[start.len()..]
                .find(close)
                .map(|end| at + start.len() + end + close.len());
            *open = end.is_none();
            let end = end.unwrap_or(line.len());
            out.push((at..end, Token::Comment));
            at = end;
            continue;
        }
        if byte == b'"' || byte == b'\'' || byte == b'`' {
            let Some((end, token)) = quoted(lang, bytes, at) else {
                at += 1;
                continue;
            };
            out.push((at..end, token));
            at = end;
            continue;
        }
        if byte.is_ascii_digit() {
            let end = at + word_len(&bytes[at..], true);
            out.push((at..end, Token::Number));
            at = end;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$' {
            let end = at + 1 + word_len(&bytes[at + 1..], false);
            if let Some(token) = word(lang, line, at..end, at == first_word) {
                out.push((at..end, token));
            }
            at = end;
            continue;
        }
        at += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

// The quoted run opening at `at` and what it is: a string, or a JSON
// key; `None` for a lone quote, a Rust lifetime (`'a`) among them.
fn quoted(lang: &Lang, bytes: &[u8], at: usize) -> Option<(usize, Token)> {
    let byte = bytes[at];
    let end = escaped_quote_end(bytes, at)
        .filter(|end| byte != b'\'' || !std::ptr::eq(lang, &RUST) || end - at <= 4)?;
    let key = bytes[end..].iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b':');
    let token = if key && std::ptr::eq(lang, &JSON) {
        Token::Property
    } else {
        Token::String
    };
    Some((end, token))
}

// What the word at `range` is: a config key leading its line, a keyword,
// a capitalised type, or nothing to colour.
fn word(lang: &Lang, line: &str, range: Range<usize>, leads: bool) -> Option<Token> {
    let bytes = line.as_bytes();
    let next = bytes[range.end..]
        .iter()
        .find(|byte| !byte.is_ascii_whitespace());
    if lang.keyed && leads && matches!(next, Some(b'=' | b':')) {
        Some(Token::Property)
    } else if lang.keywords.contains(&&line[range.clone()]) {
        Some(Token::Keyword)
    } else if bytes[range.start].is_ascii_uppercase() && !lang.keyed {
        Some(Token::Type)
    } else {
        None
    }
}

fn word_len(bytes: &[u8], number: bool) -> usize {
    bytes
        .iter()
        .position(|byte| {
            !(byte.is_ascii_alphanumeric() || *byte == b'_' || (number && *byte == b'.'))
        })
        .unwrap_or(bytes.len())
}

// The end (exclusive) of the quoted run opening at `at`, backslash-escaped.
fn escaped_quote_end(bytes: &[u8], at: usize) -> Option<usize> {
    let quote = bytes[at];
    let mut index = at + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

// The calm palette's link and accent are ink, so keywords take the one
// tint it has; types and keys are ink set heavier instead of a hue.
pub fn style(token: Token, theme: &Theme) -> HighlightStyle {
    let color = match token {
        Token::Keyword => Some(theme.agent),
        Token::String => Some(theme.success),
        Token::Number => Some(theme.warning),
        Token::Comment => Some(theme.muted),
        Token::Type | Token::Property => None,
    };
    HighlightStyle {
        color,
        font_weight: matches!(token, Token::Type | Token::Property).then_some(FontWeight::SEMIBOLD),
        font_style: (token == Token::Comment).then_some(FontStyle::Italic),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words<'a>(line: &'a str, tokens: &[(Range<usize>, Token)]) -> Vec<(&'a str, Token)> {
        tokens
            .iter()
            .map(|(range, token)| (&line[range.clone()], *token))
            .collect()
    }

    #[test]
    fn rust_keywords_types_strings_numbers_and_comments() {
        let lines = [
            "pub fn run(x: Vec<u8>) -> &'a str { \"hi\" } // done",
            "let n = 42;",
        ];
        let tokens = tokens("src/lib.rs", &lines);
        assert_eq!(
            words(lines[0], &tokens[0]),
            vec![
                ("pub", Token::Keyword),
                ("fn", Token::Keyword),
                ("Vec", Token::Type),
                ("\"hi\"", Token::String),
                ("// done", Token::Comment),
            ]
        );
        assert_eq!(
            words(lines[1], &tokens[1]),
            vec![("let", Token::Keyword), ("42", Token::Number)]
        );
    }

    #[test]
    fn a_block_comment_carries_across_lines() {
        let lines = ["a /* open", "still", "done */ fn"];
        let tokens = tokens("x.ts", &lines);
        assert_eq!(words(lines[1], &tokens[1]), vec![("still", Token::Comment)]);
        assert_eq!(
            words(lines[2], &tokens[2]),
            vec![("done */", Token::Comment)],
            "fn is not a JS keyword"
        );
    }

    #[test]
    fn a_fenced_blocks_language_picks_its_table() {
        let lines = ["fn main() {}"];
        assert_eq!(
            words(lines[0], &tokens("rust", &lines)[0]),
            vec![("fn", Token::Keyword)]
        );
        assert!(tokens("", &lines)[0].is_empty(), "no language is plain");
    }

    #[test]
    fn config_keys_json_keys_shell_and_markdown() {
        let toml = ["name = \"forge\" # the crate", "[deps]"];
        let t = tokens("Cargo.toml", &toml);
        assert_eq!(
            words(toml[0], &t[0]),
            vec![
                ("name", Token::Property),
                ("\"forge\"", Token::String),
                ("# the crate", Token::Comment),
            ]
        );
        let yaml = ["  enabled: true"];
        assert_eq!(
            words(yaml[0], &tokens("ci.yml", &yaml)[0]),
            vec![("enabled", Token::Property), ("true", Token::Keyword)]
        );
        let json = [r#"{"key": "value", "n": 1.5, "ok": null}"#];
        assert_eq!(
            words(json[0], &tokens("a.json", &json)[0]),
            vec![
                ("\"key\"", Token::Property),
                ("\"value\"", Token::String),
                ("\"n\"", Token::Property),
                ("1.5", Token::Number),
                ("\"ok\"", Token::Property),
                ("null", Token::Keyword),
            ]
        );
        let sh = ["if [ -n \"$X\" ]; then echo hi; fi # end"];
        let words_sh = words(sh[0], &tokens("run.sh", &sh)[0]);
        assert_eq!(words_sh[0], ("if", Token::Keyword));
        assert_eq!(words_sh.last(), Some(&("# end", Token::Comment)));
        let md = ["# Title", "text", "```rust"];
        let t = tokens("README.md", &md);
        assert_eq!(t[0], vec![(0..7, Token::Keyword)]);
        assert!(t[1].is_empty());
        assert_eq!(t[2], vec![(0..7, Token::Comment)]);
        assert!(tokens("notes.txt", &["fn x"])[0].is_empty(), "plain text");
    }
}
