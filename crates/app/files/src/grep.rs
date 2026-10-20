//! A literal pattern matched line by line. One holding newlines matches the lines it spans: the
//! text reads as its lines joined by `\n`, each line's own `\r` dropped, and a hit is the line it
//! starts on with every line it spans.
use std::collections::VecDeque;

pub struct Spans<'a, At> {
    pattern: &'a str,
    span: usize,
    window: VecDeque<(At, String)>,
}

impl<'a, At: Clone> Spans<'a, At> {
    pub fn new(pattern: &'a str) -> Self {
        Spans {
            pattern,
            span: pattern.split('\n').count(),
            window: VecDeque::new(),
        }
    }

    pub fn line(&mut self, at: At, text: &str) -> Option<(At, String)> {
        if self.span == 1 {
            return text.contains(self.pattern).then(|| (at, text.to_owned()));
        }
        let text = text.strip_suffix('\r').unwrap_or(text);
        self.window.push_back((at, text.to_owned()));
        if self.window.len() > self.span {
            self.window.pop_front();
        }
        if self.window.len() < self.span {
            return None;
        }
        let lines: Vec<&str> = self.window.iter().map(|(_, text)| text.as_str()).collect();
        let joined = lines.join("\n");
        let first = self.window[0].0.clone();
        joined.contains(self.pattern).then_some((first, joined))
    }

    pub fn gap(&mut self) {
        self.window.clear();
    }
}
