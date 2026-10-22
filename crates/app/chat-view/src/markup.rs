//! chat's paragraphs read as the shared markdown model where they spell one: a run of `| … |`
//! lines under a `|---|` row is a table, a `# ` line a heading. Chat keeps each line as typed,
//! one block per line; a view reads these at display time, as it reads list items.
use std::rc::Rc;

use chat::message::Attachment;
use chat::view::Names;
use chat::{Block, Mark, Principal, Span};
use markdown::{Marks, OnLink, Text};
use view::prelude::*;

use crate::Chat;

// What a message's blocks draw as, from the block each opens at: one of
// chat's own, or a markdown block read out of one or more paragraphs.
pub(crate) enum Piece<'a> {
    Chat(&'a Block),
    Markdown(markdown::Block),
    Artifact(String),
    Site(String),
}

pub(crate) fn pieces<'a>(
    blocks: &'a [Block],
    names: &Names,
    chain: &str,
    author: Option<&Principal>,
    sites: &dyn Fn(&Attachment) -> bool,
) -> Vec<(usize, Piece<'a>)> {
    let pages = !matches!(author, Some(Principal::Foreign(_)));
    let mut out = Vec::new();
    let mut at = 0;
    while at < blocks.len() {
        if let Some((rows, taken)) = table(&blocks[at..], names) {
            out.push((at, Piece::Markdown(markdown::Block::Table(rows))));
            at += taken;
            continue;
        }
        let piece = match heading(&blocks[at], names) {
            Some((level, text)) => Piece::Markdown(markdown::Block::Heading(level, text)),
            None => match artifact(&blocks[at], chain).filter(|_| pages) {
                Some(link) => Piece::Artifact(link),
                None => Piece::Chat(&blocks[at]),
            },
        };
        out.push((at, piece));
        let site = site_page(&blocks[at], chain, sites).filter(|_| pages);
        out.extend(site.map(|link| (at, Piece::Site(link))));
        at += 1;
    }
    out
}

// A press on a link or a mention in a markdown block, opened as one in a
// paragraph is (`message::line`).
pub(crate) fn opens(cx: &mut Context<Chat>) -> OnLink {
    Rc::new(cx.listener(|chat, target: &String, _, cx| {
        cx.notify();
        chat.open_pressed(target.clone(), cx);
    }))
}

fn artifact(block: &Block, chain: &str) -> Option<String> {
    let spans = match block {
        Block::Paragraph(spans) => spans,
        Block::Attachment(file) if file.path.rsplit('/').next().is_some_and(names_a_page) => {
            return crate::links::file_link(chain, &file.path);
        }
        _ => return None,
    };
    let mut url = None;
    for span in spans {
        let link = span.marks.iter().find_map(|mark| match mark {
            Mark::Link(target) => Some(target),
            _ => None,
        });
        match link {
            Some(link) if url.is_none_or(|url| url == link) => url = Some(link),
            Some(_) => return None,
            None if span.text.trim().is_empty() => {}
            None => return None,
        }
    }
    let url = url?;
    let link = ducklink::Link::parse(url).ok()?;
    if chain.parse::<ducklink::ChainId>().ok()? != link.chain {
        return None;
    }
    let page = match (link.program.as_str(), link.tail.as_slice()) {
        (::files::PROGRAM, [.., name]) => names_a_page(name),
        ("blobs", [hex]) => hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()),
        _ => false,
    };
    page.then(|| url.clone())
}

fn site_page(block: &Block, chain: &str, sites: &dyn Fn(&Attachment) -> bool) -> Option<String> {
    let Block::Attachment(file) = block else {
        return None;
    };
    sites(file)
        .then(|| crate::links::file_link(chain, &format!("{}/index.html", file.path)))
        .flatten()
}

fn names_a_page(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".html") || name.ends_with(".htm")
}

// The table the blocks open with, GFM's: a row, a separator row of as many
// cells, and every row under them, each padded or cut to the header's
// width; and how many blocks it took.
fn table(blocks: &[Block], names: &Names) -> Option<(Vec<Vec<Text>>, usize)> {
    let [Block::Paragraph(head), Block::Paragraph(rule), rest @ ..] = blocks else {
        return None;
    };
    let head = row(head, names)?;
    let rule = row(rule, names)?;
    if rule.len() != head.len() || !rule.iter().all(|cell| is_rule(&cell.text)) {
        return None;
    }
    let width = head.len();
    let mut rows = vec![head];
    for block in rest {
        let Block::Paragraph(spans) = block else {
            break;
        };
        let Some(mut cells) = row(spans, names) else {
            break;
        };
        cells.resize(width, Text::default());
        rows.push(cells);
    }
    let taken = rows.len() + 1;
    Some((rows, taken))
}

// A line that opens with `|`, as its cells: split at each `|` that is not
// escaped (`\|` is a pipe in a cell), the empty cell before the first and
// after a closing `|` dropped, each cell trimmed with its marks kept.
fn row(spans: &[Span], names: &Names) -> Option<Vec<Text>> {
    if !spans.first()?.text.trim_start().starts_with('|') {
        return None;
    }
    let mut cells = vec![Text::default()];
    for span in spans {
        let (words, marks) = run(span, names);
        let mut rest = words.as_str();
        while let Some(at) = cell_end(rest) {
            push(&mut cells, &rest[..at], &marks);
            cells.push(Text::default());
            rest = &rest[at + 1..];
        }
        push(&mut cells, rest, &marks);
    }
    cells.remove(0);
    if cells.last().is_some_and(|cell| cell.text.trim().is_empty()) {
        cells.pop();
    }
    let cells: Vec<Text> = cells.iter().map(Text::trimmed).collect();
    (!cells.is_empty()).then_some(cells)
}

fn push(cells: &mut [Text], words: &str, marks: &Marks) {
    if let Some(cell) = cells.last_mut() {
        cell.push(&words.replace("\\|", "|"), marks.clone());
    }
}

// Where the cell that `text` starts in ends: its first unescaped `|`.
fn cell_end(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        match c {
            '|' if !escaped => return Some(at),
            '\\' => escaped = !escaped,
            _ => escaped = false,
        }
    }
    None
}

// A separator cell: dashes, a colon either side for its alignment.
fn is_rule(cell: &str) -> bool {
    let cell = cell.strip_prefix(':').unwrap_or(cell);
    let cell = cell.strip_suffix(':').unwrap_or(cell);
    !cell.is_empty() && cell.bytes().all(|byte| byte == b'-')
}

// A paragraph opening with one to six `#` and a space: its level and the
// rest of it.
fn heading(block: &Block, names: &Names) -> Option<(usize, Text)> {
    let Block::Paragraph(spans) = block else {
        return None;
    };
    let first = spans.first().filter(|span| span.marks.is_empty())?;
    let level = first.text.bytes().take_while(|byte| *byte == b'#').count();
    let rest = first.text[level..].strip_prefix(' ')?;
    if !(1..=6).contains(&level) {
        return None;
    }
    let mut text = Text::default();
    text.push(rest, Marks::default());
    for span in &spans[1..] {
        let (words, marks) = run(span, names);
        text.push(&words, marks);
    }
    let text = text.trimmed();
    (!text.text.is_empty()).then_some((level, text))
}

// A span's words as the reader sees them (a mention by its name) and its
// marks: a link, or a mention of an account, presses through as chat's own
// runs do (`Chat::open_pressed`), the link first.
fn run(span: &Span, names: &Names) -> (String, Marks) {
    let mut words = span.text.clone();
    let mut marks = Marks::default();
    let mut mention = None;
    for mark in &span.marks {
        match mark {
            Mark::Bold => marks.bold = true,
            Mark::Italic => marks.italic = true,
            Mark::Code => marks.code = true,
            Mark::Link(url) => marks.link = Some(url.clone()),
            Mark::Mention(principal) => {
                words = names.mention(principal);
                if let Principal::Account(account) = principal {
                    mention = Some(account.to_string());
                }
            }
        }
    }
    marks.link = marks.link.or(mention);
    (words, marks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Block {
        Block::paragraph(text)
    }

    fn cells(row: &[Text]) -> Vec<&str> {
        row.iter().map(|cell| cell.text.as_str()).collect()
    }

    // j-131 message 15's blocks as chat keeps them: one paragraph per typed
    // line, the `total` cell's word bold.
    fn j131_message_15() -> Vec<Block> {
        let total = Block::Paragraph(vec![
            Span::plain("| "),
            Span {
                text: "total".into(),
                marks: vec![Mark::Bold],
            },
            Span::plain(" (phys_footprint, matches Activity Monitor) | 104.8 | 151.3 | +46.5 |"),
        ]);
        vec![
            plain("Here is the first macOS table."),
            plain("| footprint share (MiB) | idle | chat + 5 rooms | Δ |"),
            plain("|---|---|---|---|"),
            total,
            plain("| heap, dirty pages of the malloc regions | 30.9 | 60.6 | +29.7 |"),
            plain("| ↳ of which the allocator says is in use | 24.3 | 35.7 | +11.4 |"),
            plain("| GPU (OS regions: IOAccelerator + IOSurface), dirty | 54.0 | 63.5 | +9.5 |"),
            plain("| untagged anonymous (wasm memory), dirty | 1.4 | 8.4 | +7.0 |"),
            plain("| ↳ chat/1 view, linear memory resident | — | 7.2 | |"),
            plain("| notifiers | 0.5 | 0.5 | |"),
            plain("| pictures + rasters | 0.0 | 0.8 | |"),
            plain("Not in the footprint:"),
            plain("- compiled view code: 104 MiB of clean mapped pages"),
        ]
    }

    #[test]
    fn a_posted_table_reads_as_one_table_its_marks_kept() {
        let blocks = j131_message_15();
        let pieces = pieces(&blocks, &Names::empty(), "", None, &|_| false);
        let opens: Vec<usize> = pieces.iter().map(|(at, _)| *at).collect();
        assert_eq!(opens, [0, 1, 11, 12], "the table takes blocks 1 to 10");
        let (_, Piece::Markdown(markdown::Block::Table(rows))) = &pieces[1] else {
            panic!("block 1 opens a table");
        };
        assert_eq!(rows.len(), 9, "the header and 8 rows, the separator gone");
        assert!(rows.iter().all(|row| row.len() == 4));
        assert_eq!(
            cells(&rows[0]),
            ["footprint share (MiB)", "idle", "chat + 5 rooms", "Δ"]
        );
        assert_eq!(
            cells(&rows[1]),
            [
                "total (phys_footprint, matches Activity Monitor)",
                "104.8",
                "151.3",
                "+46.5"
            ]
        );
        let bold = Marks {
            bold: true,
            ..Marks::default()
        };
        assert_eq!(rows[1][0].runs, vec![(0..5, bold)]);
        assert_eq!(
            cells(&rows[6]),
            ["↳ chat/1 view, linear memory resident", "—", "7.2", ""]
        );
        assert!(matches!(pieces[2].1, Piece::Chat(Block::Paragraph(_))));
        assert!(matches!(pieces[3].1, Piece::Chat(Block::Paragraph(_))));
    }

    #[test]
    fn pipes_without_a_separator_under_them_stay_paragraphs() {
        let blocks = vec![plain("| a | b |"), plain("| 1 | 2 |"), plain("|---|")];
        let pieces = pieces(&blocks, &Names::empty(), "", None, &|_| false);
        assert_eq!(pieces.len(), 3);
        assert!(
            pieces
                .iter()
                .all(|(_, piece)| matches!(piece, Piece::Chat(_)))
        );
    }

    #[test]
    fn an_escaped_pipe_stays_in_its_cell_and_a_short_row_is_padded() {
        let blocks = vec![plain("| a \\| b | c |"), plain("|:--|--:|"), plain("| 1")];
        let pieces = pieces(&blocks, &Names::empty(), "", None, &|_| false);
        let [(_, Piece::Markdown(markdown::Block::Table(rows)))] = &pieces[..] else {
            panic!("one table");
        };
        assert_eq!(cells(&rows[0]), ["a | b", "c"]);
        assert_eq!(cells(&rows[1]), ["1", ""]);
    }

    #[test]
    fn a_hash_and_a_space_opens_a_heading_and_a_room_name_does_not() {
        let blocks = vec![
            plain("## Results"),
            plain("#general"),
            plain("####### seven"),
            plain("#"),
        ];
        let pieces = pieces(&blocks, &Names::empty(), "", None, &|_| false);
        let (_, Piece::Markdown(markdown::Block::Heading(2, text))) = &pieces[0] else {
            panic!("a level-2 heading");
        };
        assert_eq!(text.text, "Results");
        assert!(
            pieces[1..]
                .iter()
                .all(|(_, piece)| matches!(piece, Piece::Chat(_)))
        );
    }

    #[test]
    fn a_mention_in_a_cell_is_its_name_and_presses_through_to_its_account() {
        let blocks = vec![
            Block::Paragraph(vec![
                Span::plain("| "),
                Span {
                    text: "<@8>".into(),
                    marks: vec![Mark::Mention(Principal::Account(8))],
                },
                Span::plain(" |"),
            ]),
            plain("|---|"),
        ];
        let pieces = pieces(&blocks, &Names::empty(), "", None, &|_| false);
        let [(_, Piece::Markdown(markdown::Block::Table(rows)))] = &pieces[..] else {
            panic!("one table");
        };
        let cell = &rows[0][0];
        assert_eq!(cell.text, Names::empty().mention(&Principal::Account(8)));
        assert_eq!(cell.runs[0].1.link.as_deref(), Some("8"));
    }
}
