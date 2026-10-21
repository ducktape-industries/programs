//! An ask: a question put to accounts with two to four options, one of them the default its asker
//! goes on with, as one message of ordinary blocks every reader shows as text. `blocks` writes it,
//! `read` finds one in a message (typed by hand too), and `answer` reads a reply's words as an option.
use std::ops::RangeInclusive;

use crate::text::fold;
use crate::{AccountNumber, Block, ListMarker, Mark, Principal, Span, list_item};

pub const TAG: &str = "ask";
pub const OPTIONS: RangeInclusive<usize> = 2..=4;
const DETAIL: &str = " \u{2014} ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub to: Vec<AccountNumber>,
    pub question: String,
    pub options: Vec<Choice>,
    pub default: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub label: String,
    pub detail: String,
}

pub fn check(ask: &Ask) -> Result<(), String> {
    if ask.to.is_empty() {
        return Err("an ask is put to at least one account: name them in to".into());
    }
    let question = ask.question.trim();
    let one_line = !question.is_empty() && !question.contains('\n');
    if !one_line {
        return Err("an ask's question is one line of words".into());
    }
    let offered = ask.options.len();
    if !OPTIONS.contains(&offered) {
        let (least, most) = (OPTIONS.start(), OPTIONS.end());
        return Err(format!(
            "an ask offers {least} to {most} options, not {offered}"
        ));
    }
    let mut labels = Vec::new();
    for choice in &ask.options {
        let label = choice.label.trim();
        let lines = label.contains('\n') || choice.detail.contains('\n');
        let unworded = label.is_empty() || lines;
        if unworded {
            return Err(format!(
                "option {label:?}: a label is one line of words, and so is its detail"
            ));
        }
        if label.contains(DETAIL) {
            return Err(format!(
                "option {label:?}: a label holds no {DETAIL:?}, which opens its detail"
            ));
        }
        let folded = fold(label);
        if labels.contains(&folded) {
            return Err(format!("option {label:?} is offered twice"));
        }
        labels.push(folded);
    }
    if ask.default >= offered {
        return Err(format!(
            "the default is one of the options: 0 to {}, not {}",
            offered - 1,
            ask.default
        ));
    }
    Ok(())
}

pub fn blocks(ask: &Ask) -> Vec<Block> {
    let mut head = Vec::new();
    for account in &ask.to {
        head.push(Span {
            text: format!("<@{account}>"),
            marks: vec![Mark::Mention(Principal::Account(*account))],
        });
        head.push(Span::plain(" "));
    }
    head.push(Span {
        text: ask.question.trim().to_owned(),
        marks: vec![Mark::Bold],
    });
    head.push(Span::plain(format!(" #{TAG}")));
    let mut blocks = vec![Block::Paragraph(head)];
    for (at, choice) in ask.options.iter().enumerate() {
        let marks = match at == ask.default {
            true => vec![Mark::Bold],
            false => Vec::new(),
        };
        let mut spans = vec![
            Span::plain(format!("{}. ", at + 1)),
            Span {
                text: choice.label.trim().to_owned(),
                marks,
            },
        ];
        let detail = choice.detail.trim();
        if !detail.is_empty() {
            spans.push(Span::plain(format!("{DETAIL}{detail}")));
        }
        blocks.push(Block::Paragraph(spans));
    }
    let default = ask
        .options
        .get(ask.default)
        .map_or("", |choice| choice.label.trim());
    blocks.push(Block::Paragraph(vec![Span {
        text: format!(
            "Going with {default} unless you pick otherwise: reply with a number, an option or your own words."
        ),
        marks: vec![Mark::Italic],
    }]));
    blocks
}

pub fn read(blocks: &[Block]) -> Option<Ask> {
    let [Block::Paragraph(head), rest @ ..] = blocks else {
        return None;
    };
    let (to, question) = header(head)?;
    let mut options: Vec<Choice> = Vec::new();
    let mut defaults = Vec::new();
    for block in rest {
        let Block::Paragraph(spans) = block else {
            break;
        };
        let Some((number, choice, strong)) = option(spans) else {
            break;
        };
        if number != options.len() as u64 + 1 {
            return None;
        }
        if strong {
            defaults.push(options.len());
        }
        options.push(choice);
    }
    let [default] = defaults.as_slice() else {
        return None;
    };
    let ask = Ask {
        to,
        question,
        options,
        default: *default,
    };
    check(&ask).ok().map(|()| ask)
}

pub fn answer(ask: &Ask, words: &str) -> Option<usize> {
    let words = fold(words.trim());
    let named = ask
        .options
        .iter()
        .position(|choice| fold(choice.label.trim()) == words);
    let numbered = || {
        let number = words.parse::<usize>().ok()?;
        number.checked_sub(1).filter(|at| *at < ask.options.len())
    };
    named.or_else(numbered)
}

fn header(spans: &[Span]) -> Option<(Vec<AccountNumber>, String)> {
    let mut to = Vec::new();
    let mut at = 0;
    while let Some(span) = spans.get(at) {
        let mentioned = span.marks.iter().find_map(|mark| match mark {
            Mark::Mention(Principal::Account(number)) => Some(*number),
            _ => None,
        });
        match mentioned {
            Some(number) => to.push(number),
            None if span.text.trim().is_empty() => {}
            None => break,
        }
        at += 1;
    }
    let last = spans.last()?;
    let worded = last
        .marks
        .iter()
        .all(|mark| matches!(mark, Mark::Bold | Mark::Italic));
    let words: String = spans[at..].iter().map(|span| span.text.as_str()).collect();
    let tag = format!("#{TAG}");
    let question = words.trim_end().strip_suffix(tag.as_str())?;
    let apart = question.is_empty() || question.ends_with(char::is_whitespace);
    let found = worded && apart && !to.is_empty();
    found.then(|| (to, question.trim().to_owned()))
}

fn option(spans: &[Span]) -> Option<(u64, Choice, bool)> {
    let (ListMarker::Ordered(number), item) = list_item(spans)? else {
        return None;
    };
    let mut label = String::new();
    let mut detail: Option<String> = None;
    let mut strong = true;
    for span in &item {
        if let Some(detail) = &mut detail {
            detail.push_str(&span.text);
            continue;
        }
        let bold = span.marks.contains(&Mark::Bold);
        let split = match bold {
            true => None,
            false => span.text.split_once(DETAIL),
        };
        let head = split.map_or(span.text.as_str(), |(head, _)| head);
        if !head.trim().is_empty() {
            strong &= bold;
        }
        label.push_str(head);
        if let Some((_, tail)) = split {
            detail = Some(tail.to_owned());
        }
    }
    let choice = Choice {
        label: label.trim().to_owned(),
        detail: detail.unwrap_or_default().trim().to_owned(),
    };
    Some((number, choice, strong))
}
