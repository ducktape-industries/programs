//! Markdown into blocks of rich text, by pulldown-cmark: CommonMark plus
//! GFM tables, task lists, strikethrough and footnotes. Raw HTML stays the
//! text it is; an image is its alt text, quiet; nothing is fetched.
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, TextMergeStream};

use crate::{Block, Marks, Text};

enum Frame {
    Root,
    Quote,
    // the next ordinal of an ordered list
    List(Option<u64>),
    Item(String),
}

#[derive(Default)]
struct Builder {
    frames: Vec<(Frame, Vec<Block>)>,
    text: Text,
    bold: usize,
    italic: usize,
    strike: usize,
    quiet: usize,
    links: Vec<String>,
    code: Option<(String, String)>,
    table: Option<Vec<Vec<Text>>>,
}

impl Builder {
    fn marks(&self) -> Marks {
        Marks {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: false,
            quiet: self.quiet > 0,
            link: self.links.last().cloned(),
        }
    }

    fn push(&mut self, text: &str, marks: Marks) {
        self.text.push(text, marks);
    }

    // Prose outside a link: a bare `https://`, `http://` or `duck://` word
    // is a link too, as it was under chat's tokenizer.
    fn prose(&mut self, text: &str) {
        let marks = self.marks();
        if marks.link.is_some() {
            return self.push(text, marks);
        }
        let mut rest = text;
        while let Some(at) = ["https://", "http://", "duck://"]
            .iter()
            .filter_map(|scheme| rest.find(scheme))
            .min()
        {
            let end = rest[at..]
                .find(char::is_whitespace)
                .map_or(rest.len(), |len| at + len);
            let url =
                rest[at..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'', '"']);
            self.push(&rest[..at], marks.clone());
            self.push(
                url,
                Marks {
                    link: Some(url.to_owned()),
                    ..marks.clone()
                },
            );
            rest = &rest[at + url.len()..];
        }
        self.push(rest, marks);
    }

    fn take(&mut self) -> Text {
        std::mem::take(&mut self.text)
    }

    fn block(&mut self, block: Block) {
        self.frames.last_mut().expect("root frame").1.push(block);
    }

    // Loose inline text (a tight list item's) becomes a paragraph before
    // anything else opens.
    fn flush(&mut self) {
        if !self.text.text.trim().is_empty() {
            let text = self.take();
            self.block(Block::Paragraph(text));
        }
        self.text = Text::default();
    }

    fn close(&mut self) -> (Frame, Vec<Block>) {
        self.flush();
        self.frames.pop().expect("an open frame")
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { .. } => {
                self.quiet += 1;
                self.italic += 1;
                self.push("[", self.marks());
            }
            Tag::Superscript | Tag::Subscript => {}
            Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell => self.flush(),
            Tag::TableHead | Tag::TableRow => {
                if let Some(rows) = &mut self.table {
                    rows.push(Vec::new());
                }
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Vec::new());
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.frames.push((Frame::Quote, Vec::new()));
            }
            Tag::List(first) => {
                self.flush();
                self.frames.push((Frame::List(first), Vec::new()));
            }
            Tag::Item => {
                self.flush();
                let marker = match self.frames.last_mut() {
                    Some((Frame::List(Some(next)), _)) => {
                        *next += 1;
                        format!("{}.", *next - 1)
                    }
                    _ => "•".to_owned(),
                };
                self.frames.push((Frame::Item(marker), Vec::new()));
            }
            Tag::FootnoteDefinition(label) => {
                self.flush();
                self.frames
                    .push((Frame::Item(format!("[{label}]")), Vec::new()));
            }
            Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition => self.flush(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Emphasis => self.italic -= 1,
            TagEnd::Strong => self.bold -= 1,
            TagEnd::Strikethrough => self.strike -= 1,
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                if self.text.text.ends_with('[') {
                    self.push("image", self.marks());
                }
                self.push("]", self.marks());
                self.quiet -= 1;
                self.italic -= 1;
            }
            TagEnd::Heading(level) => {
                let text = self.take();
                self.block(Block::Heading(level as usize, text));
            }
            TagEnd::TableCell => {
                let text = self.take();
                if let Some(row) = self.table.as_mut().and_then(|rows| rows.last_mut()) {
                    row.push(text);
                }
            }
            TagEnd::Table => {
                let rows = self.table.take().unwrap_or_default();
                self.block(Block::Table(rows));
            }
            TagEnd::CodeBlock => {
                if let Some((lang, mut body)) = self.code.take() {
                    if body.ends_with('\n') {
                        body.pop();
                    }
                    self.block(Block::Code(lang, body));
                }
            }
            TagEnd::BlockQuote(_) => {
                let (_, blocks) = self.close();
                self.block(Block::Quote(blocks));
            }
            TagEnd::List(_) => {
                let (_, items) = self.close();
                self.frames.last_mut().expect("root frame").1.extend(items);
            }
            TagEnd::Item | TagEnd::FootnoteDefinition => {
                if let (Frame::Item(marker), blocks) = self.close() {
                    self.block(Block::Item(marker, blocks));
                }
            }
            TagEnd::HtmlBlock => {
                let len = self.text.text.trim_end().len();
                self.text.text.truncate(len);
                self.flush();
            }
            _ => self.flush(),
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => match &mut self.code {
                Some((_, body)) => body.push_str(&text),
                None => self.prose(&text),
            },
            Event::Code(text) => {
                let marks = Marks {
                    code: true,
                    ..self.marks()
                };
                self.push(&text, marks);
            }
            Event::Html(text) | Event::InlineHtml(text) => self.push(&text, self.marks()),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                self.push(&text, self.marks());
            }
            Event::FootnoteReference(label) => {
                let marks = Marks {
                    quiet: true,
                    ..self.marks()
                };
                self.push(&format!("[{label}]"), marks);
            }
            Event::SoftBreak => self.push(" ", self.marks()),
            Event::HardBreak => self.push("\n", Marks::default()),
            Event::Rule => {
                self.flush();
                self.block(Block::Rule);
            }
            Event::TaskListMarker(done) => {
                if let Some((Frame::Item(marker), _)) = self.frames.last_mut() {
                    *marker = if done { "☑" } else { "☐" }.to_owned();
                }
            }
        }
    }
}

pub fn parse(text: &str) -> Vec<Block> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let mut builder = Builder {
        frames: vec![(Frame::Root, Vec::new())],
        ..Builder::default()
    };
    for event in TextMergeStream::new(Parser::new_ext(text, options)) {
        builder.event(event);
    }
    builder.flush();
    builder
        .frames
        .pop()
        .map(|(_, blocks)| blocks)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Text {
        Text {
            text: text.into(),
            runs: Vec::new(),
        }
    }

    fn para(text: &str) -> Block {
        Block::Paragraph(plain(text))
    }

    #[test]
    fn the_block_constructs_parse_to_their_blocks() {
        let doc = "# Title\n\nOne line\ncarried on.\n\n- a\n  - nested\n- [ ] todo\n- [x] done\n\n\
                   3. third\n4. fourth\n\n> quoted\n> still\n>\n> > deeper\n\n---\n\n\
                   ```rust title\nfn main() {}\n```\n\n    indented\n\n\
                   | a | b |\n|---|:-:|\n| 1 | 2 |\n\n## Next ##";
        assert_eq!(
            parse(doc),
            vec![
                Block::Heading(1, plain("Title")),
                para("One line carried on."),
                Block::Item(
                    "•".into(),
                    vec![para("a"), Block::Item("•".into(), vec![para("nested")])]
                ),
                Block::Item("☐".into(), vec![para("todo")]),
                Block::Item("☑".into(), vec![para("done")]),
                Block::Item("3.".into(), vec![para("third")]),
                Block::Item("4.".into(), vec![para("fourth")]),
                Block::Quote(vec![
                    para("quoted still"),
                    Block::Quote(vec![para("deeper")])
                ]),
                Block::Rule,
                Block::Code("rust".into(), "fn main() {}".into()),
                Block::Code(String::new(), "indented".into()),
                Block::Table(vec![
                    vec![plain("a"), plain("b")],
                    vec![plain("1"), plain("2")],
                ]),
                Block::Heading(2, plain("Next")),
            ]
        );
    }

    #[test]
    fn an_unclosed_fence_runs_to_the_end_and_a_hash_without_space_is_text() {
        assert_eq!(
            parse("#tag\n```\nx"),
            vec![para("#tag"), Block::Code(String::new(), "x".into())]
        );
    }

    fn only(doc: &str) -> Text {
        match parse(doc).as_slice() {
            [Block::Paragraph(text)] => text.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn inline_marks_are_runs() {
        let text = only(
            "run `make dev` **now**, *or* ~~never~~, see [the docs](duck://net/forge/rfcs) \
             or https://x.example.",
        );
        assert_eq!(
            text.text,
            "run make dev now, or never, see the docs or https://x.example."
        );
        let at = |needle: &str| {
            let start = text.text.find(needle).unwrap();
            start..start + needle.len()
        };
        let marks = |f: fn(&mut Marks)| {
            let mut marks = Marks::default();
            f(&mut marks);
            marks
        };
        assert_eq!(
            text.runs,
            vec![
                (at("make dev"), marks(|m| m.code = true)),
                (at("now"), marks(|m| m.bold = true)),
                (
                    at("or never").start..at("or never").start + 2,
                    marks(|m| m.italic = true)
                ),
                (at("never"), marks(|m| m.strike = true)),
                (
                    at("the docs"),
                    Marks {
                        link: Some("duck://net/forge/rfcs".into()),
                        ..Marks::default()
                    }
                ),
                (
                    at("https://x.example"),
                    Marks {
                        link: Some("https://x.example".into()),
                        ..Marks::default()
                    }
                ),
            ]
        );
    }

    #[test]
    fn a_bare_url_links_up_to_the_whitespace_through_stray_marks() {
        let text = only("see https://x.example/~me/a and https://x.example/a*b?c=1&amp;d=2 now");
        let link = |url: &str| {
            let start = text.text.find(url).unwrap();
            (
                start..start + url.len(),
                Marks {
                    link: Some(url.into()),
                    ..Marks::default()
                },
            )
        };
        assert_eq!(
            text.runs,
            vec![
                link("https://x.example/~me/a"),
                link("https://x.example/a*b?c=1&d=2"),
            ]
        );
    }

    #[test]
    fn an_image_is_its_quiet_alt_text_and_a_footnote_its_label() {
        let blocks = parse("See ![the logo](logo.png) and ![](x.png)[^1].\n\n[^1]: A note.");
        let [Block::Paragraph(text), Block::Item(marker, note)] = blocks.as_slice() else {
            panic!("{blocks:?}")
        };
        assert_eq!(text.text, "See [the logo] and [image][1].");
        assert!(text.runs.iter().all(|(_, m)| m.quiet && m.link.is_none()));
        assert_eq!(marker, "[1]");
        assert_eq!(note, &vec![para("A note.")]);
    }

    #[test]
    fn raw_html_is_shown_as_text_never_as_markup() {
        let blocks = parse("<div align=\"center\"><b>hi</b></div>\n\nan <i>inline</i> tag");
        assert_eq!(
            blocks,
            vec![
                para("<div align=\"center\"><b>hi</b></div>"),
                para("an <i>inline</i> tag"),
            ]
        );
    }
}
