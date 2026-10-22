//! A typed `@name` in a message chat stores: as the message is posted or
//! edited, a name exactly one account holds becomes that account's mention, and
//! `@name@network` or `@18@network` that of a user of a network crosspond links.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{Error, QueryCtx};
use unicode_normalization::UnicodeNormalization;

use crate::origin::roster;
use crate::{AccountNumber, Block, Foreign, Mark, Principal, Profile, Span};

/// A name typed in a message: one of this network's, or one of a network crosspond links.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Typed {
    /// `@name`, its name compared after NFC and lowercasing.
    Local(String),
    /// `@name@network` or `@18@network`, as typed.
    Foreign { name: String, network: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Mention {
    principal: Principal,
    text: String,
}

impl Mention {
    fn account(number: AccountNumber) -> Mention {
        Mention {
            principal: Principal::Account(number),
            text: format!("<@{number}>"),
        }
    }

    fn foreign(foreign: Foreign, network: &str) -> Mention {
        Mention {
            principal: Principal::Foreign(foreign),
            text: format!("@{}@{network}", foreign.account),
        }
    }
}

pub(crate) fn resolve_typed(ctx: &QueryCtx, blocks: Vec<Block>) -> Result<Vec<Block>, Error> {
    let wanted = typed_names(&blocks);
    if wanted.is_empty() {
        return Ok(blocks);
    }
    let here = String::from_utf8_lossy(&ctx.env().chain_id).into_owned();
    let mut locals = BTreeSet::new();
    let mut elsewhere: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for typed in &wanted {
        match typed {
            Typed::Local(name) => {
                locals.insert(name.clone());
            }
            Typed::Foreign { name, network } if *network == here => {
                let numbered = name.parse::<AccountNumber>().is_ok();
                if !numbered {
                    locals.insert(name_key(name));
                }
            }
            Typed::Foreign { name, network } => {
                elsewhere
                    .entry(network.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
    }
    let holding = match locals.is_empty() {
        true => BTreeMap::new(),
        false => holders(roster(ctx)?, &locals),
    };
    let mut resolved = BTreeMap::new();
    for typed in wanted {
        let mention = match &typed {
            Typed::Local(name) => holding.get(name).copied().map(Mention::account),
            Typed::Foreign { name, network } if *network == here => {
                let number = name.parse::<AccountNumber>().ok();
                let held = holding.get(&name_key(name)).copied();
                number.or(held).map(Mention::account)
            }
            Typed::Foreign { .. } => None,
        };
        if let Some(mention) = mention {
            resolved.insert(typed, mention);
        }
    }
    for (network, names) in elsewhere {
        let Some(id) = crate::linked::network_named(ctx, &network)? else {
            continue;
        };
        for name in names {
            let account = match name.parse::<AccountNumber>() {
                Ok(number) => Some(number),
                Err(_) => crate::linked::account_named(ctx, id, &name)?,
            };
            let Some(account) = account else {
                continue;
            };
            let foreign = Foreign {
                network: id,
                account,
            };
            let typed = Typed::Foreign {
                name,
                network: network.clone(),
            };
            resolved.insert(typed, Mention::foreign(foreign, &network));
        }
    }
    Ok(resolve(blocks, &resolved))
}

pub(crate) fn typed_names(blocks: &[Block]) -> BTreeSet<Typed> {
    let mut wanted = BTreeSet::new();
    for block in blocks {
        typed_spans(spans_of(block), |typed| {
            wanted.insert(typed.clone());
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

fn resolve(blocks: Vec<Block>, resolved: &BTreeMap<Typed, Mention>) -> Vec<Block> {
    let holder = |typed: &Typed| resolved.get(typed).cloned();
    blocks
        .into_iter()
        .map(|block| match block {
            Block::Paragraph(spans) => Block::Paragraph(typed_spans(&spans, holder)),
            Block::Quote(spans) => Block::Quote(typed_spans(&spans, holder)),
            Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => block,
        })
        .collect()
}

fn spans_of(block: &Block) -> &[Span] {
    match block {
        Block::Paragraph(spans) | Block::Quote(spans) => spans,
        Block::Code { .. } | Block::Divider | Block::Attachment(_) | Block::Notice(_) => &[],
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

fn typed_spans(spans: &[Span], mut holder: impl FnMut(&Typed) -> Option<Mention>) -> Vec<Span> {
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

fn run_len(run: &[char]) -> usize {
    let run = &run[..run.iter().take_while(|c| name_char(**c)).count()];
    let dots = run.iter().rev().take_while(|c| **c == '.').count();
    run.len() - dots
}

fn typed_at(chars: &[char], at: usize, before: Option<char>) -> Option<(Typed, usize)> {
    let opens = chars[at] == '@' && before.is_none_or(|c| !name_char(c) && c != '@');
    if !opens {
        return None;
    }
    let run = &chars[at + 1..];
    let len = run_len(run);
    if len == 0 {
        return None;
    }
    let name: String = run[..len].iter().collect();
    let names_a_network = run.get(len) == Some(&'@');
    let network_len = match names_a_network {
        true => run_len(&run[len + 1..]),
        false => 0,
    };
    if network_len == 0 {
        return Some((Typed::Local(name_key(&name)), len));
    }
    let network: String = run[len + 1..len + 1 + network_len].iter().collect();
    let typed = Typed::Foreign { name, network };
    Some((typed, len + 1 + network_len))
}

fn split(
    span: &Span,
    before: Option<char>,
    holder: &mut impl FnMut(&Typed) -> Option<Mention>,
) -> Vec<Span> {
    let chars: Vec<char> = span.text.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut at = 0;
    while at < chars.len() {
        let before = at.checked_sub(1).map(|prev| chars[prev]).or(before);
        let held = typed_at(&chars, at, before)
            .and_then(|(typed, len)| holder(&typed).map(|mention| (mention, len)));
        let Some((mention, len)) = held else {
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
        let mut marks = vec![Mark::Mention(mention.principal)];
        marks.extend(span.marks.iter().cloned());
        out.push(Span {
            text: mention.text,
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
