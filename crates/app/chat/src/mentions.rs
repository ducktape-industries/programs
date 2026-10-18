//! A typed `@name` in a message chat stores: as the message is posted or
//! edited, a name exactly one account holds becomes that account's mention.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{Error, QueryCtx};
use unicode_normalization::UnicodeNormalization;

use crate::origin::roster;
use crate::{AccountNumber, Block, Mark, Principal, Profile, Span};

pub(crate) fn resolve_typed(ctx: &QueryCtx, blocks: Vec<Block>) -> Result<Vec<Block>, Error> {
    let wanted = typed_names(&blocks);
    if wanted.is_empty() {
        return Ok(blocks);
    }
    Ok(resolve(blocks, &holders(roster(ctx)?, &wanted)))
}

pub(crate) fn typed_names(blocks: &[Block]) -> BTreeSet<String> {
    let mut wanted = BTreeSet::new();
    for block in blocks {
        typed_spans(spans_of(block), |name| {
            wanted.insert(name.to_owned());
            None
        });
    }
    wanted
}

pub(crate) fn holders(
    profiles: Vec<Profile>,
    wanted: &BTreeSet<String>,
) -> BTreeMap<String, AccountNumber> {
    let mut holding: BTreeMap<String, Vec<AccountNumber>> = BTreeMap::new();
    for profile in profiles {
        let name = name_key(&profile.name);
        if wanted.contains(&name) {
            holding.entry(name).or_default().push(profile.number);
        }
    }
    holding
        .into_iter()
        .filter_map(|(name, numbers)| match numbers.as_slice() {
            [number] => Some((name, *number)),
            _ => None,
        })
        .collect()
}

pub(crate) fn resolve(blocks: Vec<Block>, holders: &BTreeMap<String, AccountNumber>) -> Vec<Block> {
    let holder = |name: &str| holders.get(name).copied();
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Paragraph(spans) => Block::Paragraph(typed_spans(&spans, holder)),
            Block::Quote(spans) => Block::Quote(typed_spans(&spans, holder)),
            Block::Code { .. } | Block::Divider | Block::Attachment(_) => block,
        })
        .collect()
}

fn spans_of(block: &Block) -> &[Span] {
    match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans,
        Block::Code { .. } | Block::Divider | Block::Attachment(_) => &[],
    }
}

pub(crate) fn name_key(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

fn name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | '.')
}

fn carries_typed_names(mark: &Mark) -> bool {
    match mark {
        Mark::Bold | Mark::Italic => true,
        Mark::Link(_) | Mark::Mention(_) | Mark::Code => false,
    }
}

fn typed_spans(spans: &[Span], mut holder: impl FnMut(&str) -> Option<AccountNumber>) -> Vec<Span> {
    let mut out = Vec::new();
    let mut before = None;
    for span in spans {
        let typed = span.marks.iter().all(carries_typed_names);
        if typed {
            out.extend(split(span, before, &mut holder));
        } else {
            out.push(span.clone());
        }
        before = span.text.chars().next_back().or(before);
    }
    out
}

fn name_len(chars: &[char], at: usize, before: Option<char>) -> Option<usize> {
    let opens = chars[at] == '@' && before.is_none_or(|c| !name_char(c) && c != '@');
    if !opens {
        return None;
    }
    let run = &chars[at + 1..];
    let run = &run[..run.iter().take_while(|c| name_char(**c)).count()];
    let dots = run.iter().rev().take_while(|c| **c == '.').count();
    let len = run.len() - dots;
    (len > 0).then_some(len)
}

fn split(
    span: &Span,
    before: Option<char>,
    holder: &mut impl FnMut(&str) -> Option<AccountNumber>,
) -> Vec<Span> {
    let chars: Vec<char> = span.text.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut at = 0;
    while at < chars.len() {
        let before = at.checked_sub(1).map(|prev| chars[prev]).or(before);
        let held = name_len(&chars, at, before).and_then(|len| {
            let name: String = chars[at + 1..at + 1 + len].iter().collect();
            holder(&name_key(&name)).map(|number| (number, len))
        });
        let Some((number, len)) = held else {
            text.push(chars[at]);
            at += 1;
            continue;
        };
        if !text.is_empty() {
            out.push(Span {
                text: std::mem::take(&mut text),
                marks: span.marks.clone(),
            });
        }
        let mut marks = vec![Mark::Mention(Principal::Account(number))];
        marks.extend(span.marks.iter().cloned());
        out.push(Span {
            text: format!("<@{number}>"),
            marks,
        });
        at += 1 + len;
    }
    let untouched = out.is_empty();
    if untouched {
        return vec![span.clone()];
    }
    if !text.is_empty() {
        out.push(Span {
            text,
            marks: span.marks.clone(),
        });
    }
    out
}
