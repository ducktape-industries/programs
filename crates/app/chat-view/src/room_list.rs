//! The rooms the reader is in, as both lists draw them: their channels (opened on this device,
//! listened to, written in or owned) under the folders a `/` in a name files, in the reader's own
//! order, then their direct rooms newest first; search finds the rest by name.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use chat::view::Names;
use chat::{Category, ChannelInfo, Entry, Kind, Principal};
use view::Point;
use view::prelude::*;
use view::wire;

use crate::Chat;
use crate::names::dm_peer_of;
use crate::queries;

// A job's room (`j-…`, under folders too: `jobs/j-097`) or a program's own
// (a change's review): what `#` offers only once something is typed.
pub(crate) fn is_work(info: &ChannelInfo) -> bool {
    chat::namespace::program(&info.channel.id).is_some()
        || label(&info.channel.name).starts_with("j-")
}

// A direct room in the list: the room and the other person.
pub(crate) struct DmEntry<'a> {
    pub info: &'a ChannelInfo,
    pub peer: u64,
}

pub(crate) struct RoomList<'a> {
    pub channels: Vec<&'a ChannelInfo>,
    pub dms: Vec<DmEntry<'a>>,
    // the channels left out, archived or not the reader's: what tells a network with no channel
    // from a reader in none yet
    pub left_out: usize,
}

// A row of the desktop's list the arrows stand on: a room, or a folder,
// with the channel named as the folder, which its row opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Spot {
    Room(String),
    Folder { path: String, own: Option<String> },
}

impl Spot {
    // what the reader's order names the row by
    pub(crate) fn entry(&self) -> Entry {
        match self {
            Spot::Room(room) => Entry::Room(room.clone()),
            Spot::Folder { path, .. } => Entry::Folder(path.clone()),
        }
    }

    // the room the row opens: none for a folder alone
    pub(crate) fn room(&self) -> Option<&str> {
        match self {
            Spot::Room(room) => Some(room),
            Spot::Folder { own, .. } => own.as_deref(),
        }
    }
}

fn folder_at(spots: &[Spot], path: &str) -> Option<usize> {
    spots
        .iter()
        .position(|spot| matches!(spot, Spot::Folder { path: held, .. } if held == path))
}

// A line of the channel list as drawn: a channel, or a folder's row.
pub(crate) enum Line<'a> {
    Room { info: &'a ChannelInfo, depth: usize },
    Folder(FolderLine<'a>),
}

impl Line<'_> {
    pub(crate) fn spot(&self) -> Spot {
        match self {
            Line::Room { info, .. } => Spot::Room(info.channel.id.clone()),
            Line::Folder(folder) => Spot::Folder {
                path: folder.path.clone(),
                own: folder.own.map(|own| own.channel.id.clone()),
            },
        }
    }
}

// A folder's row: its path (its parts joined by `/`), its last part, the
// channel named as it, how deep it sits, and while it is closed how many
// rooms inside are unread.
pub(crate) struct FolderLine<'a> {
    pub path: String,
    pub name: &'a str,
    pub own: Option<&'a ChannelInfo>,
    pub depth: usize,
    pub open: bool,
    pub unread: usize,
}

// A channel's name in parts: the folder before its first `/` and the name after it, each trimmed,
// later `/`s the name's own. A name with no `/`, or nothing on one side of the first, is one part,
// as it is.
fn segments(name: &str) -> Vec<&str> {
    match name.split_once('/') {
        Some((folder, rest)) if !folder.trim().is_empty() && !rest.trim().is_empty() => {
            vec![folder.trim(), rest.trim()]
        }
        _ => vec![name],
    }
}

// What the create form's name field asks once `change` landed, `had` its folder chip before it:
// while none is chipped, the text before a `/` just typed or pasted becomes the chip, an atomic span
// at the field's start (`wire::CHIP`); the chip deleted whole with nothing in its place (Backspace
// right after it) comes back as its text without the `/`; anything else asks nothing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Chip {
    Leave,
    Make { end: usize, id: String },
    Unmake { text: String },
}

pub(crate) fn chip(change: &wire::TextChange, had: Option<&str>) -> Chip {
    let Some(edit) = change.edit else {
        return Chip::Leave;
    };
    if chipped(change).is_some() {
        return Chip::Leave;
    }
    let (start, end) = (edit.range.start as usize, edit.range.end as usize);
    let removed = start == 0 && edit.len == 0;
    if let Some(had) = had.filter(|had| removed && end == had.len()) {
        let text = had.strip_suffix('/').unwrap_or(had);
        return Chip::Unmake {
            text: text.to_owned(),
        };
    }
    let Some(slash) = change.text.find('/') else {
        return Chip::Leave;
    };
    let folder = change.text[..slash].trim();
    let typed = (start..start + edit.len as usize).contains(&slash);
    match typed && !folder.is_empty() {
        true => Chip::Make {
            end: slash + 1,
            id: format!("{}{folder}", wire::CHIP),
        },
        false => Chip::Leave,
    }
}

// The text of the folder chip `change` holds, if any.
pub(crate) fn chipped(change: &wire::TextChange) -> Option<&str> {
    let token = change
        .tokens
        .iter()
        .find(|token| token.id.starts_with(wire::CHIP))?;
    change.text.get(token.range.range())
}

// What a channel's row says: inside a folder, its name after the folder.
pub(crate) fn label(name: &str) -> &str {
    match segments(name).as_slice() {
        [_, .., last] => last,
        _ => name,
    }
}

// The channels as their names file them, in the list's order: a folder
// stands where its first channel would.
#[derive(Default)]
struct Tree<'a> {
    top: Vec<Node<'a>>,
    folders: Vec<Folder<'a>>,
    // each folder's place in `folders`, by its path
    places: BTreeMap<String, usize>,
}

enum Node<'a> {
    Room(&'a ChannelInfo),
    Folder(usize),
}

struct Folder<'a> {
    path: String,
    name: &'a str,
    own: Option<&'a ChannelInfo>,
    nodes: Vec<Node<'a>>,
}

impl<'a> Tree<'a> {
    fn of(channels: &[&'a ChannelInfo]) -> Self {
        let paths: BTreeSet<String> = channels
            .iter()
            .flat_map(|info| {
                let parts = segments(&info.channel.name);
                (1..parts.len()).map(move |end| parts[..end].join("/"))
            })
            .collect();
        let mut tree = Tree::default();
        for &info in channels {
            let parts = segments(&info.channel.name);
            // the first channel named as a folder is the folder's own row
            if paths.contains(&parts.join("/"))
                && let Some(at) = tree.folder(&parts)
                && tree.folders[at].own.is_none()
            {
                tree.folders[at].own = Some(info);
                continue;
            }
            let dirs = parts.split_last().map_or(&[][..], |(_, dirs)| dirs);
            let above = tree.folder(dirs);
            tree.nodes(above).push(Node::Room(info));
        }
        tree
    }

    // The folder at `parts`, made where it is not yet, with each folder
    // above it, after what its parent holds; none for no parts.
    fn folder(&mut self, parts: &[&'a str]) -> Option<usize> {
        let mut above = None;
        for (depth, &name) in parts.iter().enumerate() {
            let path = parts[..=depth].join("/");
            let at = match self.places.get(&path).copied() {
                Some(at) => at,
                None => {
                    let at = self.folders.len();
                    self.places.insert(path.clone(), at);
                    self.folders.push(Folder {
                        path,
                        name,
                        own: None,
                        nodes: Vec::new(),
                    });
                    self.nodes(above).push(Node::Folder(at));
                    at
                }
            };
            above = Some(at);
        }
        above
    }

    // Each level, the top and each folder's, in the reader's `order`: what it leaves out first, in
    // the order it stood, then what it names, in its order.
    fn arrange(&mut self, order: &[Entry]) {
        let top = std::mem::take(&mut self.top);
        self.top = self.ranked(top, order);
        for at in 0..self.folders.len() {
            let nodes = std::mem::take(&mut self.folders[at].nodes);
            self.folders[at].nodes = self.ranked(nodes, order);
        }
    }

    fn ranked(&self, mut nodes: Vec<Node<'a>>, order: &[Entry]) -> Vec<Node<'a>> {
        let rank = |node: &Node| {
            let entry = self.entry(node);
            order.iter().position(|listed| *listed == entry)
        };
        nodes.sort_by_cached_key(|node| rank(node).map_or((false, 0), |at| (true, at)));
        nodes
    }

    fn entry(&self, node: &Node) -> Entry {
        match *node {
            Node::Room(info) => Entry::Room(info.channel.id.clone()),
            Node::Folder(at) => Entry::Folder(self.folders[at].path.clone()),
        }
    }

    // the entries of the level that holds `entry`, as drawn
    fn level(&self, entry: &Entry) -> Option<Vec<Entry>> {
        let folders = self.folders.iter().map(|folder| &folder.nodes);
        std::iter::once(&self.top)
            .chain(folders)
            .map(|nodes| {
                nodes
                    .iter()
                    .map(|node| self.entry(node))
                    .collect::<Vec<_>>()
            })
            .find(|level| level.contains(entry))
    }

    fn nodes(&mut self, folder: Option<usize>) -> &mut Vec<Node<'a>> {
        match folder {
            Some(at) => &mut self.folders[at].nodes,
            None => &mut self.top,
        }
    }

    // `nodes` as lines `depth` folders deep, a closed folder's left out
    fn lines(
        &self,
        nodes: &[Node<'a>],
        depth: usize,
        closed: &BTreeSet<String>,
        unread: &dyn Fn(&ChannelInfo) -> bool,
        lines: &mut Vec<Line<'a>>,
    ) {
        for node in nodes {
            let folder = match *node {
                Node::Room(info) => {
                    lines.push(Line::Room { info, depth });
                    continue;
                }
                Node::Folder(at) => &self.folders[at],
            };
            let open = !closed.contains(&folder.path);
            let hidden = match open {
                true => 0,
                false => {
                    let inside = self.rooms(&folder.nodes);
                    inside.into_iter().filter(|&info| unread(info)).count()
                }
            };
            lines.push(Line::Folder(FolderLine {
                path: folder.path.clone(),
                name: folder.name,
                own: folder.own,
                depth,
                open,
                unread: hidden,
            }));
            if open {
                self.lines(&folder.nodes, depth + 1, closed, unread, lines);
            }
        }
    }

    // every channel in `nodes`, each folder's own included
    fn rooms(&self, nodes: &[Node<'a>]) -> Vec<&'a ChannelInfo> {
        let mut rooms = Vec::new();
        for node in nodes {
            match *node {
                Node::Room(info) => rooms.push(info),
                Node::Folder(at) => {
                    let folder = &self.folders[at];
                    rooms.extend(folder.own);
                    rooms.extend(self.rooms(&folder.nodes));
                }
            }
        }
        rooms
    }
}

// The reader's `order` once `level`'s entry at `from` moved to `to`: the level as drawn, written
// whole after what the order says of every other level.
pub(crate) fn moved(order: &[Entry], level: &[Entry], from: usize, to: usize) -> Vec<Entry> {
    let mut level = level.to_vec();
    let entry = level.remove(from);
    level.insert(to.min(level.len()), entry);
    let mut order: Vec<Entry> = order
        .iter()
        .filter(|entry| !level.contains(entry))
        .cloned()
        .collect();
    order.extend(level);
    order
}

// Where layout put each channel row of the sidebar.
pub(crate) type Drawn = BTreeMap<Entry, Bounds<Pixels>>;

// A sidebar row held down: the row, and the row of its level it lands on once let go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Drag {
    pub entry: Entry,
    pub onto: Option<Entry>,
}

pub(crate) struct DmChoice {
    pub peer: u64,
    pub name: String,
    pub agent: bool,
}

impl DmChoice {
    fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || self.name.to_lowercase().contains(query)
            || self.peer.to_string().contains(query)
    }
}

fn person_or_agent(names: &Names, peer: u64) -> bool {
    matches!(
        names.kind(&Principal::Account(peer)),
        Some(
            Kind::User
                | Kind::Program {
                    category: Category::Agent,
                    ..
                }
        )
    )
}

impl Chat {
    pub(crate) fn room_list(&self) -> RoomList<'_> {
        let mut list = self.held_rooms();
        list.dms.sort_by_key(|dm| Reverse(self.recency(dm)));
        list
    }

    // Whether the reader is in `info`'s room: opened on this device, listened to, written in, or
    // theirs.
    pub(crate) fn is_in(&self, info: &ChannelInfo) -> bool {
        let id = info.channel.id.as_str();
        let owner = self.my_account().map(Principal::Account);
        self.opened.contains(id)
            || self.listening.contains(id)
            || self.posted.contains(id)
            || owner.as_ref() == Some(&info.channel.owner)
    }

    // `channels` as the list draws them: each under the folders its name
    // gives, in the reader's order, a closed folder's rooms left out
    pub(crate) fn lines<'a>(&self, channels: &[&'a ChannelInfo]) -> Vec<Line<'a>> {
        let mut tree = Tree::of(channels);
        tree.arrange(&self.order);
        let mut lines = Vec::new();
        let unread = |info: &ChannelInfo| self.unread(info);
        tree.lines(&tree.top, 0, &self.closed_folders, &unread, &mut lines);
        lines
    }

    // The entries of the level of the lists that holds `entry`, as drawn: none for a direct room.
    pub(crate) fn level(&self, entry: &Entry) -> Option<Vec<Entry>> {
        let list = self.held_rooms();
        let mut tree = Tree::of(&list.channels);
        tree.arrange(&self.order);
        tree.level(entry)
    }

    // Whether `entry` has a place before it and after it in its level.
    pub(crate) fn steps(&self, entry: &Entry) -> (bool, bool) {
        let Some(level) = self.level(entry) else {
            return (false, false);
        };
        let at = level.iter().position(|held| held == entry);
        at.map_or((false, false), |at| (at > 0, at + 1 < level.len()))
    }

    // `entry` one place up or down its level.
    pub(crate) fn step(&mut self, entry: &Entry, up: bool, cx: &mut Context<Self>) {
        let Some(at) = self
            .level(entry)
            .and_then(|level| level.iter().position(|held| held == entry))
        else {
            return;
        };
        let to = match up {
            true => at.checked_sub(1),
            false => Some(at + 1),
        };
        if let Some(to) = to {
            self.place(entry, to, cx);
        }
    }

    // `entry` put at `to` in its level, and the reader's whole order written.
    pub(crate) fn place(&mut self, entry: &Entry, to: usize, cx: &mut Context<Self>) {
        let Some(level) = self.level(entry) else {
            return;
        };
        let Some(from) = level.iter().position(|held| held == entry) else {
            return;
        };
        if from == to || to >= level.len() {
            return;
        }
        self.order = moved(&self.order, &level, from, to);
        cx.notify();
        let order = self.order.clone();
        self.submit(chat::Op::SetRoomOrder { order }, cx);
    }

    // The row under `at`, of the rows `drawn` now.
    fn row_at(&self, at: Point<Pixels>, drawn: &[Entry]) -> Option<Entry> {
        let under = |entry: &&Entry| {
            let bounds = self.drawn_rows.get(*entry);
            bounds.is_some_and(|bounds| bounds.contains(&at))
        };
        drawn.iter().find(under).cloned()
    }

    // A left press on the channel rows: the row under it is held.
    pub(crate) fn hold_row(&mut self, at: Point<Pixels>, drawn: &[Entry]) {
        self.drag = self
            .row_at(at, drawn)
            .map(|entry| Drag { entry, onto: None });
    }

    // The pointer moved while a row is held: over another row of its level, that row is where it
    // lands; over itself or another level, nowhere; between rows, where it was.
    pub(crate) fn drag_row(&mut self, at: Point<Pixels>, drawn: &[Entry], cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        let Some(over) = self.row_at(at, drawn) else {
            return;
        };
        let level = self.level(&drag.entry).unwrap_or_default();
        let onto = (over != drag.entry && level.contains(&over)).then_some(over);
        if let Some(drag) = self.drag.as_mut().filter(|drag| drag.onto != onto) {
            drag.onto = onto;
            cx.notify();
        }
    }

    // Let go: a row held and dragged onto another of its level takes that row's place.
    pub(crate) fn drop_row(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        cx.notify();
        let Drag {
            entry,
            onto: Some(onto),
        } = drag
        else {
            return;
        };
        let level = self.level(&entry).unwrap_or_default();
        if let Some(to) = level.iter().position(|held| *held == onto) {
            self.place(&entry, to, cx);
        }
    }

    // The sidebar row of `entry` was laid out at `bounds`.
    pub(crate) fn drawn_row(&mut self, entry: Entry, bounds: Bounds<Pixels>) {
        self.drawn_rows.insert(entry, bounds);
    }

    // Where the held row lands on `entry`'s row: after it (`true`) when it comes from above.
    pub(crate) fn landing(&self, entry: &Entry) -> Option<bool> {
        let drag = self.drag.as_ref()?;
        if drag.onto.as_ref() != Some(entry) {
            return None;
        }
        let level = self.level(entry)?;
        let at = |entry: &Entry| level.iter().position(|held| held == entry);
        Some(at(&drag.entry)? < at(entry)?)
    }

    // The folder at `path` opened or closed, and kept so on the device.
    pub(crate) fn fold(&mut self, path: &str, open: bool, cx: &mut Context<Self>) {
        let moved = match open {
            true => self.closed_folders.remove(path),
            false => self.closed_folders.insert(path.to_owned()),
        };
        if moved {
            self.save_folders(cx);
        }
    }

    pub(crate) fn toggle_folder(&mut self, path: &str, cx: &mut Context<Self>) {
        let open = self.closed_folders.contains(path);
        self.fold(path, open, cx);
    }

    // A press on the row at `spot`: its room opens, or a folder alone opens
    // or closes.
    pub(crate) fn press_spot(&mut self, spot: &Spot, cx: &mut Context<Self>) {
        match spot {
            Spot::Folder { path, own: None } => self.toggle_folder(path, cx),
            Spot::Folder {
                own: Some(room), ..
            }
            | Spot::Room(room) => self.choose(room.clone(), cx),
        }
    }

    fn recency(&self, dm: &DmEntry) -> u64 {
        let created = dm.info.channel.created_at;
        self.dm_heads
            .get(&dm.info.channel.id)
            .and_then(|head| head.time)
            .unwrap_or(created)
    }

    // The rooms both lists hold: the channels the reader is in and their direct rooms. An archived
    // room leaves the lists as it is archived, and any other, a change's or a job's too, is found by
    // search; a link or the open room still reaches either through `channels`.
    fn held_rooms(&self) -> RoomList<'_> {
        let mine = self.my_account();
        let mut list = RoomList {
            channels: Vec::new(),
            dms: Vec::new(),
            left_out: 0,
        };
        for info in self.channels.ready().into_iter().flatten() {
            let id = info.channel.id.as_str();
            let direct = chat::dm_peers(id).is_some();
            if info.channel.archived || !(direct || self.is_in(info)) {
                list.left_out += usize::from(!direct);
                continue;
            }
            if !direct {
                list.channels.push(info);
                continue;
            }
            list.dms.extend(
                mine.and_then(|mine| dm_peer_of(mine, id))
                    .map(|peer| DmEntry { info, peer }),
            );
        }
        list
    }

    pub(crate) fn settle_dm_heads(&mut self, cx: &mut Context<Self>) {
        let Some(mine) = self.my_account() else {
            return;
        };
        let heads: Vec<(String, u64)> = self
            .channels
            .ready()
            .into_iter()
            .flatten()
            .filter(|info| dm_peer_of(mine, &info.channel.id).is_some())
            .map(|info| (info.channel.id.clone(), info.head_seq))
            .collect();
        self.dm_heads
            .retain(|id, _| heads.iter().any(|(held, _)| held == id));
        let viewer = self.viewer();
        for (id, seq) in heads {
            let head = self.dm_heads.entry(id.clone()).or_default();
            let moved = seq > 0 && head.seq != seq;
            if !moved {
                continue;
            }
            head.seq = seq;
            let newest = queries::newest(cx.host(), id.clone(), seq, viewer.clone());
            head.reading = Some(cx.land(newest, move |chat, time, cx| {
                let Some(head) = chat.dm_heads.get_mut(&id).filter(|head| head.seq == seq) else {
                    return;
                };
                head.reading = None;
                match time {
                    Ok(time) => {
                        head.time = time.or(head.time);
                        cx.notify();
                    }
                    Err(refusal) => cx.log_refused("a direct room's newest message", &refusal),
                }
            }));
        }
    }

    pub(crate) fn dm_choices(&self, query: &str) -> Vec<DmChoice> {
        let (Some(mine), Some(names)) = (self.my_account(), self.names.ready()) else {
            return Vec::new();
        };
        let query = query.trim().to_lowercase();
        let mut choices: Vec<DmChoice> = names
            .people()
            .filter(|peer| *peer != mine && person_or_agent(names, *peer))
            .map(|peer| {
                let principal = Principal::Account(peer);
                DmChoice {
                    peer,
                    name: names.member(&principal),
                    agent: crate::message::agent(names, &principal),
                }
            })
            .filter(|choice| choice.matches(&query))
            .collect();
        choices.sort_by_key(|choice| (choice.name.to_lowercase(), choice.peer));
        choices
    }

    pub(crate) fn begin_dm(&mut self) {
        self.dm_pick.get_or_insert_default();
    }

    pub(crate) fn open_dm(&mut self, peer: u64, cx: &mut Context<Self>) {
        let Some(mine) = self.my_account() else {
            return;
        };
        self.dm_pick = None;
        self.choose(chat::dm_channel_id(mine, peer), cx);
    }

    pub(crate) fn open_first_dm(&mut self, cx: &mut Context<Self>) {
        let first = self
            .dm_pick
            .as_ref()
            .map(|pick| self.dm_choices(&pick.query.text()))
            .and_then(|choices| choices.first().map(|choice| choice.peer));
        let Some(peer) = first else {
            return;
        };
        self.open_dm(peer, cx);
    }

    /// The row the room list's arrows are on: where they were left, else
    /// the open room, else the first.
    pub(crate) fn rooms_at(&self, spots: &[Spot]) -> usize {
        let open = self.room.as_ref().map(|room| Spot::Room(room.id.clone()));
        let Some(spot) = self.rooms_cursor.as_ref().or(open.as_ref()) else {
            return 0;
        };
        let at = |room: &str| spots.iter().position(|listed| listed.room() == Some(room));
        let row = match spot {
            Spot::Folder { path, .. } => folder_at(spots, path),
            // a room a closed folder hides hands its row to that folder
            Spot::Room(id) => at(id)
                .or_else(|| self.folding(id, spots))
                .or_else(|| self.in_place_of(id, &at)),
        };
        row.unwrap_or(0)
    }

    // the row of the innermost folder drawn that holds the channel `id`
    fn folding(&self, id: &str, spots: &[Spot]) -> Option<usize> {
        let parts = segments(&self.info(id)?.channel.name);
        (1..parts.len())
            .rev()
            .find_map(|end| folder_at(spots, &parts[..end].join("/")))
    }

    // A room the list leaves out (an archived channel) hands its row to the
    // next listed channel after it in the program's order, else to the last
    // one before it.
    fn in_place_of(&self, id: &str, at: &impl Fn(&str) -> Option<usize>) -> Option<usize> {
        let channels: Vec<&str> = self
            .channels
            .ready()?
            .iter()
            .map(|info| info.channel.id.as_str())
            .filter(|room| chat::dm_peers(room).is_none())
            .collect();
        let place = channels.iter().position(|room| *room == id)?;
        let (before, after) = channels.split_at(place);
        let next = after.iter().copied().find_map(at);
        let last = || before.iter().rev().copied().find_map(at);
        next.or_else(last)
    }
}
