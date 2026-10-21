//! Rust items of a text, found by a token scanner rather than a parser: each `fn`, `struct`, `enum`,
//! `union`, `trait`, `type`, `const`, `static`, `mod`, `macro_rules!` and `impl` at the top, in inline
//! modules and in impl and trait blocks, from the line its attributes or doc comment open to its end.
use ::program::{Error, invalid, not_found};

use super::lines::{Picked, range};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Item {
    pub name: String,
    pub first: u64,
    pub last: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Lexeme {
    Word(String),
    Mark(char),
    Doc,
}

struct Token {
    lexeme: Lexeme,
    line: u64,
}

fn word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn string(chars: &[char], mut at: usize, raw: Option<usize>, line: &mut u64) -> usize {
    while let Some(&c) = chars.get(at) {
        if c == '\n' {
            *line += 1;
        }
        match (c, raw) {
            ('\\', None) => {
                if chars.get(at + 1) == Some(&'\n') {
                    *line += 1;
                }
                at += 2;
                continue;
            }
            ('"', None) => return at + 1,
            ('"', Some(hashes)) if (1..=hashes).all(|n| chars.get(at + n) == Some(&'#')) => {
                return at + 1 + hashes;
            }
            _ => {}
        }
        at += 1;
    }
    at
}

fn quote(chars: &[char], at: usize) -> usize {
    match (chars.get(at + 1), chars.get(at + 2)) {
        (Some('\\'), _) => {
            let mut end = at + 3;
            while chars.get(end).is_some_and(|&c| c != '\'' && c != '\n') {
                end += 1;
            }
            end + 1
        }
        (Some(_), Some('\'')) => at + 3,
        _ => {
            let mut end = at + 1;
            while chars.get(end).copied().is_some_and(word_char) {
                end += 1;
            }
            end
        }
    }
}

fn comment(chars: &[char], mut at: usize, line: &mut u64) -> usize {
    let mut depth = 0usize;
    while let Some(&c) = chars.get(at) {
        match (c, chars.get(at + 1)) {
            ('/', Some('*')) => {
                depth += 1;
                at += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return at;
                }
            }
            ('\n', _) => {
                *line += 1;
                at += 1;
            }
            _ => at += 1,
        }
    }
    at
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut line = 1;
    let mut at = 0;
    while let Some(&c) = chars.get(at) {
        let next = chars.get(at + 1).copied();
        match c {
            '\n' => {
                line += 1;
                at += 1;
            }
            c if c.is_whitespace() => at += 1,
            '/' if next == Some('/') => {
                let doc = chars.get(at + 2) == Some(&'/') && chars.get(at + 3) != Some(&'/');
                if doc {
                    found.push(Token {
                        lexeme: Lexeme::Doc,
                        line,
                    });
                }
                while chars.get(at).is_some_and(|&c| c != '\n') {
                    at += 1;
                }
            }
            '/' if next == Some('*') => {
                let doc = chars.get(at + 2) == Some(&'*')
                    && !matches!(chars.get(at + 3), Some('*' | '/'));
                if doc {
                    found.push(Token {
                        lexeme: Lexeme::Doc,
                        line,
                    });
                }
                at = comment(&chars, at, &mut line);
            }
            '"' => at = string(&chars, at + 1, None, &mut line),
            '\'' => at = quote(&chars, at),
            c if word_char(c) => {
                let start = at;
                while chars.get(at).copied().is_some_and(word_char) {
                    at += 1;
                }
                let word: String = chars[start..at].iter().collect();
                let raw = matches!(word.as_str(), "r" | "br" | "cr");
                let hashes = chars[at..].iter().take_while(|&&c| c == '#').count();
                let prefixed_string = hashes == 0 && matches!(word.as_str(), "b" | "c");
                let byte_char = hashes == 0 && word == "b";
                let raw_identifier = |c: char| word == "r" && hashes == 1 && word_char(c);
                match chars.get(at + hashes) {
                    Some('"') if raw => {
                        at = string(&chars, at + hashes + 1, Some(hashes), &mut line);
                    }
                    Some('"') if prefixed_string => {
                        at = string(&chars, at + 1, None, &mut line);
                    }
                    Some('\'') if byte_char => at = quote(&chars, at),
                    Some(&c) if raw_identifier(c) => {
                        let start = at + 1;
                        at = start;
                        while chars.get(at).copied().is_some_and(word_char) {
                            at += 1;
                        }
                        found.push(Token {
                            lexeme: Lexeme::Word(chars[start..at].iter().collect()),
                            line,
                        });
                    }
                    _ => found.push(Token {
                        lexeme: Lexeme::Word(word),
                        line,
                    }),
                }
            }
            c => {
                found.push(Token {
                    lexeme: Lexeme::Mark(c),
                    line,
                });
                at += 1;
            }
        }
    }
    found
}

struct Scan<'a> {
    tokens: &'a [Token],
    at: usize,
    items: Vec<Item>,
}

impl<'a> Scan<'a> {
    fn word(&self, at: usize) -> Option<&'a str> {
        let tokens: &'a [Token] = self.tokens;
        match &tokens.get(at)?.lexeme {
            Lexeme::Word(word) => Some(word),
            _ => None,
        }
    }

    fn mark(&self, at: usize) -> Option<char> {
        match self.tokens.get(at)?.lexeme {
            Lexeme::Mark(mark) => Some(mark),
            _ => None,
        }
    }

    fn group(&self, mut at: usize) -> usize {
        let mut depth = 0usize;
        while at < self.tokens.len() {
            match self.mark(at) {
                Some('(' | '[' | '{') => depth += 1,
                Some(')' | ']' | '}') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return at + 1;
                    }
                }
                _ => {}
            }
            at += 1;
        }
        at
    }

    fn level(&mut self, prefix: &str) {
        let tokens = self.tokens;
        while let Some(token) = tokens.get(self.at) {
            if token.lexeme == Lexeme::Mark('}') {
                self.at += 1;
                return;
            }
            self.statement(prefix);
        }
    }

    fn statement(&mut self, prefix: &str) {
        let tokens = self.tokens;
        let mut first = None;
        while let Some(token) = tokens.get(self.at) {
            let line = token.line;
            match &token.lexeme {
                Lexeme::Mark('}') => return,
                Lexeme::Mark(';') => {
                    self.at += 1;
                    return;
                }
                Lexeme::Mark('#') if self.mark(self.at + 1) == Some('!') => {
                    self.at += 2;
                    if self.mark(self.at) == Some('[') {
                        self.at = self.group(self.at);
                    }
                    return;
                }
                Lexeme::Mark('#') => {
                    first.get_or_insert(line);
                    self.at += 1;
                    if self.mark(self.at) == Some('[') {
                        self.at = self.group(self.at);
                    }
                }
                Lexeme::Mark('{') => {
                    self.at = self.group(self.at);
                    return;
                }
                Lexeme::Mark('(' | '[') => {
                    first.get_or_insert(line);
                    self.at = self.group(self.at);
                }
                Lexeme::Word(word) => {
                    let first = *first.get_or_insert(line);
                    if self.item(word, prefix, first) {
                        return;
                    }
                    self.at += 1;
                }
                Lexeme::Doc | Lexeme::Mark(_) => {
                    first.get_or_insert(line);
                    self.at += 1;
                }
            }
        }
    }

    fn named(&self, word: &str) -> Option<(String, Option<String>)> {
        let at = self.at;
        match word {
            "impl" => {
                let (head, outer) = self.impl_head(at + 1);
                Some((format!("impl {head}"), Some(outer)))
            }
            "macro_rules" if self.mark(at + 1) == Some('!') => {
                Some((self.word(at + 2)?.to_owned(), None))
            }
            "const"
                if matches!(
                    self.word(at + 1),
                    Some("fn" | "unsafe" | "async" | "extern")
                ) =>
            {
                None
            }
            "union" if !matches!(self.mark(at + 2), Some('{' | '<')) => None,
            "fn" | "struct" | "enum" | "union" | "trait" | "type" | "const" | "static" | "mod" => {
                let skip = usize::from(word == "static" && self.word(at + 1) == Some("mut"));
                let name = self.word(at + 1 + skip)?.to_owned();
                let inner = matches!(word, "trait" | "mod").then(|| name.clone());
                Some((name, inner))
            }
            _ => None,
        }
    }

    fn item(&mut self, word: &str, prefix: &str, first: u64) -> bool {
        let Some((name, inner)) = self.named(word) else {
            return false;
        };
        let to_semicolon = matches!(word, "const" | "static" | "type");
        let index = self.items.len();
        self.items.push(Item {
            name: format!("{prefix}{name}"),
            first,
            last: first,
        });
        let mut at = self.at + 1;
        let mut angle = 0usize;
        while at < self.tokens.len() {
            let opens_body = !to_semicolon && angle == 0;
            match self.mark(at) {
                Some(';') => {
                    at += 1;
                    break;
                }
                Some('<') => {
                    angle += 1;
                    at += 1;
                }
                Some('>') => {
                    if !self.arrow(at) {
                        angle = angle.saturating_sub(1);
                    }
                    at += 1;
                }
                Some('{') if opens_body => {
                    at = match &inner {
                        Some(outer) => {
                            self.at = at + 1;
                            self.level(&format!("{prefix}{outer}::"));
                            self.at
                        }
                        None => self.group(at),
                    };
                    break;
                }
                Some('(' | '[' | '{') => at = self.group(at),
                _ => at += 1,
            }
        }
        self.at = at;
        if let Some(end) = at.checked_sub(1).and_then(|end| self.tokens.get(end)) {
            self.items[index].last = end.line;
        }
        true
    }

    fn arrow(&self, at: usize) -> bool {
        at.checked_sub(1).and_then(|at| self.mark(at)) == Some('-')
    }

    fn spelled_group(&self, from: usize, to: usize) -> String {
        let mut text = String::new();
        let mut spaced = false;
        for token in &self.tokens[from..to] {
            match &token.lexeme {
                Lexeme::Word(word) => {
                    let apart = spaced || text.ends_with(word_char);
                    if apart {
                        text.push(' ');
                    }
                    text.push_str(word);
                    spaced = false;
                }
                Lexeme::Mark(mark @ (',' | ';')) => {
                    text.push(*mark);
                    spaced = true;
                }
                Lexeme::Mark(mark) => {
                    let opens_apart = spaced && matches!(*mark, '(' | '[' | '&');
                    if opens_apart {
                        text.push(' ');
                    }
                    text.push(*mark);
                    spaced = false;
                }
                Lexeme::Doc => {}
            }
        }
        text
    }

    fn impl_head(&self, mut at: usize) -> (String, String) {
        let mut angle = 0usize;
        let mut head = String::new();
        let mut outer = String::new();
        while let Some(token) = self.tokens.get(at) {
            let outside_angles = angle == 0;
            let at_where = outside_angles && self.word(at) == Some("where");
            let at_path = outside_angles && self.mark(at + 1) == Some(':');
            let joins_head = !head.is_empty() && !head.ends_with("::");
            match &token.lexeme {
                Lexeme::Mark('{' | ';') if outside_angles => break,
                _ if at_where => break,
                Lexeme::Mark('<') => angle += 1,
                Lexeme::Mark('>') if !self.arrow(at) => angle = angle.saturating_sub(1),
                Lexeme::Mark('(' | '[') if angle == 0 => {
                    let end = self.group(at);
                    let group = self.spelled_group(at, end);
                    if joins_head {
                        head.push(' ');
                    }
                    head.push_str(&group);
                    outer = group;
                    at = end;
                    continue;
                }
                Lexeme::Mark('(' | '[') => {
                    at = self.group(at);
                    continue;
                }
                Lexeme::Mark(':') if at_path => {
                    head.push_str("::");
                    at += 2;
                    continue;
                }
                Lexeme::Mark('!') if angle == 0 => {
                    if !head.is_empty() {
                        head.push(' ');
                    }
                    head.push('!');
                }
                Lexeme::Word(word) if outside_angles => {
                    let spaced_word = joins_head && !head.ends_with('!');
                    if spaced_word {
                        head.push(' ');
                    }
                    head.push_str(word);
                    outer = word.clone();
                }
                _ => {}
            }
            at += 1;
        }
        (head, outer)
    }
}

pub(crate) fn items(text: &str) -> Vec<Item> {
    let tokens = tokens(text);
    let mut scan = Scan {
        tokens: &tokens,
        at: 0,
        items: Vec::new(),
    };
    while scan.at < tokens.len() {
        scan.level("");
    }
    scan.items
}

fn spelled(name: &str) -> String {
    let mut angle = 0usize;
    let mut kept = String::new();
    let mut before = ' ';
    for c in name.chars() {
        match c {
            '<' => angle += 1,
            '>' if before != '-' => angle = angle.saturating_sub(1),
            c if angle == 0 => kept.push(c),
            _ => {}
        }
        before = c;
    }
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn canon(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut kept = String::new();
    for (at, &c) in chars.iter().enumerate() {
        let between_words = at > 0
            && chars.get(at + 1).copied().is_some_and(word_char)
            && kept.ends_with(word_char);
        let one_space = between_words && !kept.ends_with(' ');
        if !c.is_whitespace() {
            kept.push(c);
        } else if one_space {
            kept.push(' ');
        }
    }
    kept
}

fn at_lines(item: &Item) -> String {
    format!("lines {}-{}", item.first, item.last)
}

pub(crate) fn item(text: &str, name: &str) -> Result<Picked, Error> {
    let wanted = spelled(name);
    if wanted.is_empty() {
        return Err(invalid(
            "symbol names an item, like main, Config::new or impl Config",
        ));
    }
    let all = items(text);
    let wanted_canon = canon(&wanted);
    let found: Vec<&Item> = all
        .iter()
        .filter(|item| canon(&item.name) == wanted_canon)
        .collect();
    match found.as_slice() {
        [item] => range(text, item.first, item.last),
        [] => {
            let tail = wanted_canon.rsplit("::").next().unwrap_or(&wanted_canon);
            let near: Vec<String> = all
                .iter()
                .filter(|item| canon(&item.name).rsplit("::").next() == Some(tail))
                .map(|item| format!("{} ({})", item.name, at_lines(item)))
                .collect();
            Err(not_found(match near.is_empty() {
                true => format!("no item of the file is named {wanted}; symbols: true lists them"),
                false => format!(
                    "no item of the file is named {wanted}; it holds {}",
                    near.join(", ")
                ),
            }))
        }
        many => {
            let at: Vec<String> = many.iter().copied().map(at_lines).collect();
            Err(invalid(format!(
                "{wanted} names {} items, at {}; read one by its lines",
                many.len(),
                at.join(" and ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r##"//! A header.
#![allow(dead_code)]
use std::fmt;

/** The config. */
#[derive(Debug)]
pub struct Config {
    name: String,
}

pub struct Pair(u8, u8);

impl Config {
    pub fn new(name: &str) -> Config {
        let brace = "}";
        let raw = r#"} "{ "#;
        let open = '{';
        Config { name: name.into() }
    }

    fn named<'a>(&'a self) -> &'a str {
        /* a } in /* a nested */ comment */
        &self.name
    }
}

impl<T: fmt::Debug> fmt::Display for Wrap<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

const LIMIT: [u8; 2] = [1, 2];

macro_rules! twice {
    ($e:expr) => { $e; $e };
}

#[cfg(test)]
mod tests {
    fn new() {}
}

#[cfg(unix)]
fn native() {}
#[cfg(not(unix))]
fn native() {}
"##;

    fn outline(text: &str) -> Vec<(String, u64, u64)> {
        items(text)
            .into_iter()
            .map(|item| (item.name, item.first, item.last))
            .collect()
    }

    #[test]
    fn every_item_kind_is_found_with_its_attributes_and_its_end_past_strings_chars_and_comments() {
        let expected = [
            ("Config", 5, 9),
            ("Pair", 11, 11),
            ("impl Config", 13, 25),
            ("Config::new", 14, 19),
            ("Config::named", 21, 24),
            ("impl fmt::Display for Wrap", 27, 31),
            ("Wrap::fmt", 28, 30),
            ("LIMIT", 33, 33),
            ("twice", 35, 37),
            ("tests", 39, 42),
            ("tests::new", 41, 41),
            ("native", 44, 45),
            ("native", 46, 47),
        ];
        let expected: Vec<(String, u64, u64)> = expected
            .into_iter()
            .map(|(name, first, last)| (name.to_owned(), first, last))
            .collect();
        assert_eq!(outline(FIXTURE), expected);
    }

    #[test]
    fn an_item_is_read_by_its_name_with_generics_left_out() {
        let new = item(FIXTURE, "Config::new").unwrap();
        assert_eq!((new.first, new.last, new.total), (14, 19, 47));
        assert!(new.text.starts_with("    pub fn new(") && new.text.ends_with("    }\n"));
        assert_eq!(item(FIXTURE, "impl Config").unwrap().last, 25);
        let display = item(FIXTURE, " impl  fmt::Display for Wrap<T> ").unwrap();
        assert_eq!((display.first, display.last), (27, 31));
    }

    #[test]
    fn a_name_of_several_items_or_of_none_is_refused_naming_what_the_file_holds() {
        let twice = item(FIXTURE, "native").unwrap_err();
        assert!(
            twice
                .message
                .contains("2 items, at lines 44-45 and lines 46-47"),
            "{twice:?}"
        );
        let near = item(FIXTURE, "new").unwrap_err();
        assert!(
            near.message
                .contains("Config::new (lines 14-19), tests::new (lines 41-41)"),
            "{near:?}"
        );
        let none = item(FIXTURE, "missing").unwrap_err();
        assert!(
            none.message.contains("symbols: true lists them"),
            "{none:?}"
        );
        assert!(item(FIXTURE, "<T>").is_err());
    }

    #[test]
    fn a_const_generic_block_opens_no_body_and_a_grouped_self_type_names_its_impl() {
        let text = "impl<const N: usize> G<{ N }> {\n    fn n() -> usize {\n        N\n    }\n}\nfn f<const N: usize>() -> Foo<{ N }> {\n    Foo\n}\nimpl Described for () {\n    fn tools() {}\n}\nimpl<const N: usize> KeyCodec for [u8; N] {}\nimpl KeyCodec for () {}\nimpl Pair for (u8, &str) {}\n";
        let expected: Vec<(String, u64, u64)> = [
            ("impl G", 1, 5),
            ("G::n", 2, 4),
            ("f", 6, 8),
            ("impl Described for ()", 9, 11),
            ("()::tools", 10, 10),
            ("impl KeyCodec for [u8; N]", 12, 12),
            ("impl KeyCodec for ()", 13, 13),
            ("impl Pair for (u8, &str)", 14, 14),
        ]
        .into_iter()
        .map(|(name, first, last)| (name.to_owned(), first, last))
        .collect();
        assert_eq!(outline(text), expected);
        let array = item(text, "impl KeyCodec for [u8;N]").unwrap();
        assert_eq!((array.first, array.last), (12, 12));
        assert_eq!(item(text, "impl G<{ N }>").unwrap().last, 5);
    }

    #[test]
    fn raw_identifiers_byte_strings_and_lifetimes_do_not_open_or_close_an_item() {
        let text = "fn r#match() {}\nconst B: &[u8] = b\"}\";\nstatic C: &str = br#\"{\"#;\nstruct L<'a>(&'a u8, char);\nconst Q: u8 = b'{';\nfn last() {}\n";
        let expected: Vec<(String, u64, u64)> = [
            ("match", 1, 1),
            ("B", 2, 2),
            ("C", 3, 3),
            ("L", 4, 4),
            ("Q", 5, 5),
            ("last", 6, 6),
        ]
        .into_iter()
        .map(|(name, first, last)| (name.to_owned(), first, last))
        .collect();
        assert_eq!(outline(text), expected);
    }
}
