//! Chat's `duck://` links: a room or a message to open, the route the app
//! hands back when one is opened, and the rooms a message names.
use std::collections::BTreeMap;
use std::ops::Range;

use ::chat::{Block, Mark, Span};

/// `duck://<chain>/chat/<channel>[/<seq>]`: chat's own tail, the channel
/// percent-encoded by `ducklink` like any segment (`forge:web:3` →
/// `forge%3Aweb%3A3`); none without a chain.
pub fn channel_link(chain: &str, channel: &str, seq: Option<u64>) -> Option<String> {
    let seq = seq.map(|seq| seq.to_string());
    let mut tail = vec![channel];
    tail.extend(seq.as_deref());
    ducklink::mint(chain, ::chat::PROGRAM, &tail)
}

pub(crate) fn file_link(chain: &str, path: &str) -> Option<String> {
    let tail: Vec<&str> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    ducklink::mint(chain, ::files::PROGRAM, &tail)
}

/// Where a program's room (`forge:web:3`) is shown by its program:
/// `duck://<chain>/forge/web/3`, the room id's own path
/// ([`chat::namespace::program`]). None for a room people opened, or no chain yet.
pub fn program_link(chain: &str, channel: &str) -> Option<String> {
    let program = ::chat::namespace::program(channel)?;
    let path: Vec<&str> = channel[program.len() + 1..].split(':').collect();
    ducklink::mint(chain, program, &path)
}

/// `duck://<chain>/<calls>/<agent>/chat/<channel>`: where the program an
/// agent's script calls shows its work in a room, by the place it keeps it
/// under (`chat/<channel>`), the channel percent-encoded like any segment.
/// None without a chain.
pub fn work_record(chain: &str, calls: &str, agent: u64, channel: &str) -> Option<String> {
    let agent = agent.to_string();
    ducklink::mint(chain, calls, &[&agent, ::chat::PROGRAM, channel])
}

/// `duck://explorer/block/<height>`: the host's short link to a view on
/// this connection, Explorer at that block.
pub fn block_link(height: u64) -> String {
    format!("duck://explorer/block/{height}")
}

/// The room and message a route handed to this view names:
/// `<channel>[/<seq>]`, delivered decoded, the seq 0 when there is none.
pub fn route_target(route: &str) -> Option<(String, u64)> {
    let mut parts = route.splitn(2, '/');
    let channel = parts.next()?.to_owned();
    let seq = parts.next().and_then(|seq| seq.parse().ok()).unwrap_or(0);
    (!channel.is_empty()).then_some((channel, seq))
}

pub fn room_route(chain: &str, url: &str) -> Option<(String, u64)> {
    let link = ducklink::Link::parse(url).ok()?;
    let tail: Vec<&str> = link.tail.iter().map(String::as_str).collect();
    if ducklink::mint(chain, ::chat::PROGRAM, &tail).as_deref() != Some(url) {
        return None;
    }
    route_target(&link.tail.join("/"))
}

// The rooms a message names, read against the reader's list on this chain
// (by id, each one's name): a link to one shows its name now, and a `#word`
// that is exactly one room's name links to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rooms {
    chain: String,
    names: BTreeMap<String, String>,
}

impl Rooms {
    pub fn new(chain: &str, rooms: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            chain: chain.to_owned(),
            names: rooms.into_iter().collect(),
        }
    }

    pub fn blocks(&self, blocks: Vec<Block>) -> Vec<Block> {
        blocks
            .into_iter()
            .map(|block| match block {
                Block::Paragraph(spans) => Block::Paragraph(self.spans(&spans)),
                Block::Quote(spans) => Block::Quote(self.spans(&spans)),
                block => block,
            })
            .collect()
    }

    fn spans(&self, spans: &[Span]) -> Vec<Span> {
        let mut out = Vec::new();
        for span in spans {
            let link = span.marks.iter().find_map(|mark| match mark {
                Mark::Link(url) => Some(url),
                _ => None,
            });
            match link {
                _ if span.marks.contains(&Mark::Code) => out.push(span.clone()),
                Some(url) => out.push(self.labelled(span, url)),
                None => self.tagged(span, &mut out),
            }
        }
        out
    }

    fn labelled(&self, span: &Span, url: &str) -> Span {
        let name = room_route(&self.chain, url)
            .filter(|(_, seq)| *seq == 0)
            .and_then(|(room, _)| self.names.get(&room));
        match name {
            Some(name) => Span {
                text: format!("#{name}"),
                marks: span.marks.clone(),
            },
            None => span.clone(),
        }
    }

    fn tagged(&self, span: &Span, out: &mut Vec<Span>) {
        let part = |range: Range<usize>, marks: Vec<Mark>| Span {
            text: span.text[range].to_owned(),
            marks,
        };
        let mut from = 0;
        for (range, word) in ::chat::tag_ranges(&span.text) {
            let link = self
                .named(word)
                .and_then(|room| channel_link(&self.chain, room, None));
            let Some(link) = link else {
                continue;
            };
            if from < range.start {
                out.push(part(from..range.start, span.marks.clone()));
            }
            let mut marks = span.marks.clone();
            marks.push(Mark::Link(link));
            from = range.end;
            out.push(part(range, marks));
        }
        if from == 0 || from < span.text.len() {
            out.push(part(from..span.text.len(), span.marks.clone()));
        }
    }

    fn named(&self, word: &str) -> Option<&str> {
        let word = word.to_lowercase();
        let mut named = self
            .names
            .iter()
            .filter(|(_, name)| name.to_lowercase() == word);
        let (room, _) = named.next()?;
        named.next().is_none().then_some(room.as_str())
    }
}

/// A pressed mention (an account number) becomes `duck://<chain>/identity/<n>`,
/// the link the app opens; any other link is already one and passes through.
/// None when no link can be minted (no chain yet).
pub fn pressed_link(link: String, chain: &str) -> Option<String> {
    match link.parse::<u64>() {
        Ok(account) => ducklink::mint(chain, identity::PROGRAM, &[&account.to_string()]),
        Err(_) => Some(link),
    }
}

pub(crate) fn notice_link(chain: &str, link: &str) -> Option<String> {
    match link.contains("://") {
        true => Some(link.to_owned()),
        false => ducklink::on_chain(chain, link),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_keep_their_shapes() {
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "general", Some(42)).as_deref(),
            Some("duck://testnet-0a1b2c3d/chat/general/42")
        );
        assert_eq!(channel_link("", "general", None), None);
        assert_eq!(
            pressed_link("7".into(), "testnet#0a1b2c3d").as_deref(),
            Some("duck://testnet-0a1b2c3d/identity/7")
        );
        assert_eq!(pressed_link("7".into(), ""), None);
        assert_eq!(
            program_link("testnet#0a1b2c3d", "forge:web:3").as_deref(),
            Some("duck://testnet-0a1b2c3d/forge/web/3")
        );
        assert_eq!(program_link("testnet#0a1b2c3d", "general"), None);
        assert_eq!(
            work_record("testnet#0a1b2c3d", "harness", 12, "general").as_deref(),
            Some("duck://testnet-0a1b2c3d/harness/12/chat/general")
        );
        let link = work_record("testnet#0a1b2c3d", "harness", 12, "forge:web:3").unwrap();
        assert_eq!(
            link,
            "duck://testnet-0a1b2c3d/harness/12/chat/forge%3Aweb%3A3"
        );
        assert_eq!(
            ducklink::Link::parse(&link).unwrap().tail,
            ["12", "chat", "forge:web:3"]
        );
        assert_eq!(work_record("", "harness", 12, "general"), None);
    }

    #[test]
    fn rooms_name_their_links_and_link_their_names() {
        let chain = "testnet#0a1b2c3d";
        let listed = [
            ("general", "General"),
            ("r1", "dup"),
            ("r2", "Dup"),
            ("forge:web:3", "Review web#3"),
        ];
        let rooms = Rooms::new(chain, listed.map(|(id, name)| (id.into(), name.into())));
        let general = channel_link(chain, "general", None).unwrap();
        let message = channel_link(chain, "general", Some(4)).unwrap();
        assert_eq!(room_route(chain, &message), Some(("general".into(), 4)));
        assert_eq!(room_route("other#00", &general), None);
        let text =
            format!("[#old]({general}) and #General, #dup, #none `#general` [#x]({message})");
        let blocks = rooms.blocks(::chat::parse_message(&text));
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("one paragraph")
        };
        let drawn: Vec<(&str, Option<&str>)> = spans
            .iter()
            .map(|span| {
                let link = span.marks.iter().find_map(|mark| match mark {
                    Mark::Link(url) => Some(url.as_str()),
                    _ => None,
                });
                (span.text.as_str(), link)
            })
            .collect();
        assert_eq!(
            drawn,
            [
                ("#General", Some(general.as_str())),
                (" and ", None),
                ("#General", Some(general.as_str())),
                (", #dup, #none ", None),
                ("#general", None),
                (" ", None),
                ("#x", Some(message.as_str())),
            ]
        );
    }

    #[test]
    fn a_channel_link_round_trips_through_the_apps_route() {
        let long = format!("forge:{}:{}", "r".repeat(37), u64::MAX);
        for channel in [
            "general",
            "dm-3-5",
            "forge:big-history:3",
            "forge:my.lib:12",
            "a.2E",
            "보고서 #1",
            "50% off?",
            long.as_str(),
        ] {
            let link = channel_link("testnet#0a1b2c3d", channel, Some(42)).unwrap();
            let route = ducklink::Link::parse(&link).unwrap().tail.join("/");
            assert_eq!(route_target(&route), Some((channel.to_owned(), 42)));
        }
        assert_eq!(
            channel_link("testnet#0a1b2c3d", "forge:web:3", None).as_deref(),
            Some("duck://testnet-0a1b2c3d/chat/forge%3Aweb%3A3")
        );
        assert_eq!(route_target("design"), Some(("design".into(), 0)));
        assert_eq!(
            route_target("forge:web:3/7"),
            Some(("forge:web:3".into(), 7))
        );
    }

    #[test]
    fn a_notice_link_takes_the_sessions_chain_or_stands_whole() {
        let chain = "testnet#0a1b2c3d";
        assert_eq!(
            notice_link(chain, "forge/programs/213").as_deref(),
            Some("duck://testnet-0a1b2c3d/forge/programs/213")
        );
        assert_eq!(notice_link("", "forge/programs/213"), None);
        assert_eq!(
            notice_link(chain, "https://pods.example/builds-1").as_deref(),
            Some("https://pods.example/builds-1")
        );
        assert_eq!(notice_link(chain, "forge:x/1"), None);
    }
}
