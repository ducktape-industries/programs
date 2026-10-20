//! Markdown as a document every view draws the same way: [`Block`]s of rich [`Text`],
//! a small [`highlight`]er, and both designs' drawing ([`desktop`], [`phone`]), a pressed link
//! handed to the caller's [`OnLink`]. [`parse`] (feature `parse`) reads markdown text into blocks.
pub mod desktop;
pub mod highlight;
pub mod links;
#[cfg(feature = "parse")]
mod parse;
pub mod phone;

use std::ops::Range;
use std::rc::Rc;

use view::prelude::*;

#[cfg(feature = "parse")]
pub use parse::parse;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(usize, Text),
    Paragraph(Text),
    // marker (`•`, `3.`, `☐`, `☑` or a footnote's `[1]`), then its body
    Item(String, Vec<Block>),
    Code(String, String),
    Quote(Vec<Block>),
    Rule,
    // the header row first
    Table(Vec<Vec<Text>>),
}

// One run of prose and its marked ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub runs: Vec<(Range<usize>, Marks)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    // an image's alt text or a footnote reference: drawn muted
    pub quiet: bool,
    pub link: Option<String>,
}

impl Text {
    // `text` added at the end, a run of its own unless it is unmarked.
    pub fn push(&mut self, text: &str, marks: Marks) {
        let start = self.text.len();
        self.text.push_str(text);
        if marks != Marks::default() {
            self.runs.push((start..self.text.len(), marks));
        }
    }

    // The text without its leading and trailing whitespace, each run cut to
    // what is left of it.
    pub fn trimmed(&self) -> Text {
        let start = self.text.len() - self.text.trim_start().len();
        let end = start + self.text.trim().len();
        let runs = self
            .runs
            .iter()
            .filter_map(|(range, marks)| {
                let cut =
                    range.start.clamp(start, end) - start..range.end.clamp(start, end) - start;
                (!cut.is_empty()).then(|| (cut, marks.clone()))
            })
            .collect();
        Text {
            text: self.text[start..end].to_owned(),
            runs,
        }
    }
}

// What a pressed link does, given its raw destination.
pub type OnLink = Rc<dyn Fn(&String, &mut Window, &mut App)>;

// An element id from its name, so a test or the host finds it by what it says.
fn id(text: impl Into<String>) -> ElementId {
    ElementId::Name(text.into().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimming_cuts_each_run_to_what_is_left() {
        let mut text = Text::default();
        let bold = Marks {
            bold: true,
            ..Marks::default()
        };
        text.push(" ", Marks::default());
        text.push(" total", bold.clone());
        text.push(" (all) ", Marks::default());
        text.push("  ", bold.clone());
        let trimmed = text.trimmed();
        assert_eq!(trimmed.text, "total (all)");
        assert_eq!(trimmed.runs, vec![(0..5, bold)]);
    }
}
