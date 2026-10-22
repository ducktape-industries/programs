//! Literal patterns matched line by line, as written or with ASCII letters folded. One holding
//! newlines matches the lines it spans, joined by `\n` with each line's own `\r` dropped. A line's
//! hits wait until the longest pattern's span is read past it, so they come in line order.
use std::collections::VecDeque;

use crate::Fold;

pub struct Match<At> {
    pub at: At,
    pub text: String,
    pub patterns: Vec<u32>,
}

struct Line<At> {
    at: At,
    text: String,
    folded: Option<String>,
}

impl<At> Line<At> {
    fn seen(&self) -> &str {
        self.folded.as_deref().unwrap_or(&self.text)
    }
}

pub struct Spans<At> {
    fold: Fold,
    spans: Vec<(usize, Vec<(u32, String)>)>,
    most: usize,
    window: VecDeque<Line<At>>,
}

impl<At: Clone> Spans<At> {
    pub fn new(patterns: &[String], fold: Fold) -> Self {
        let mut spans: Vec<(usize, Vec<(u32, String)>)> = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            let span = pattern.split('\n').count();
            let needle = (
                index as u32,
                folded(fold, pattern).unwrap_or_else(|| pattern.clone()),
            );
            match spans.iter_mut().find(|(each, _)| *each == span) {
                Some((_, needles)) => needles.push(needle),
                None => spans.push((span, vec![needle])),
            }
        }
        spans.sort_by_key(|(span, _)| *span);
        let most = spans.last().map_or(1, |(span, _)| *span);
        Spans {
            fold,
            spans,
            most,
            window: VecDeque::new(),
        }
    }

    pub fn kept(&self) -> usize {
        self.most - 1
    }

    pub fn line(&mut self, at: At, text: &str) -> Vec<Match<At>> {
        self.window.push_back(Line {
            at,
            text: text.to_owned(),
            folded: folded(self.fold, text),
        });
        if self.window.len() < self.most {
            return Vec::new();
        }
        let found = self.judged(self.most);
        self.window.pop_front();
        found
    }

    pub fn gap(&mut self) -> Vec<Match<At>> {
        let mut found = Vec::new();
        while !self.window.is_empty() {
            found.extend(self.judged(self.window.len()));
            self.window.pop_front();
        }
        found
    }

    fn judged(&self, lines: usize) -> Vec<Match<At>> {
        let mut found = Vec::new();
        for (span, needles) in self.spans.iter().take_while(|(span, _)| *span <= lines) {
            let seen = self.joined(*span, Line::seen);
            let patterns: Vec<u32> = needles
                .iter()
                .filter(|(_, needle)| seen.contains(needle.as_str()))
                .map(|(index, _)| *index)
                .collect();
            if !patterns.is_empty() {
                found.push(Match {
                    at: self.window[0].at.clone(),
                    text: self.joined(*span, |line| line.text.as_str()),
                    patterns,
                });
            }
        }
        found
    }

    fn joined(&self, span: usize, part: impl Fn(&Line<At>) -> &str) -> String {
        if span == 1 {
            return part(&self.window[0]).to_owned();
        }
        let lines: Vec<&str> = self
            .window
            .iter()
            .take(span)
            .map(|line| {
                let text = part(line);
                text.strip_suffix('\r').unwrap_or(text)
            })
            .collect();
        lines.join("\n")
    }
}

fn folded(fold: Fold, text: &str) -> Option<String> {
    match fold {
        Fold::Exact => None,
        Fold::Ascii => Some(text.to_ascii_lowercase()),
    }
}
