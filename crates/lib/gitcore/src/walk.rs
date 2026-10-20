// History walks over the store: ancestry checks, wants-minus-haves, the paged closure a fetch lists
// (whole, or its history with the trees left to a client that expands them, or that history cut at
// a depth for a shallow fetch), path lookup.

use super::error::{Error, Result};
use crate::object::{Commit, Kind, Mode, Tag, Tree, TreeEntry};
use crate::oid::Oid;
use crate::store::{Objects, load, load_kind};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    No,
    CapReached,
}

pub(crate) fn commit_of<S: Objects + ?Sized>(store: &S, id: &Oid) -> Result<Commit> {
    let object = load_kind(store, id, Kind::Commit)?;
    Commit::parse(&object.body, id.hash())
}

pub(crate) fn tree_of<S: Objects + ?Sized>(store: &S, id: &Oid) -> Result<Tree> {
    let object = load_kind(store, id, Kind::Tree)?;
    Tree::parse(&object.body, id.hash())
}

pub fn is_ancestor<S: Objects + ?Sized>(
    store: &S,
    ancestor: &Oid,
    descendant: &Oid,
    cap: usize,
) -> Result<Verdict> {
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    queue.push_back(*descendant);
    visited.insert(*descendant);
    while let Some(id) = queue.pop_front() {
        let found = id == *ancestor;
        if found {
            return Ok(Verdict::Yes);
        }
        let over_cap = visited.len() > cap;
        if over_cap {
            return Ok(Verdict::CapReached);
        }
        for parent in commit_of(store, &id)?.parents {
            let first_visit = visited.insert(parent);
            if first_visit {
                queue.push_back(parent);
            }
        }
    }
    Ok(Verdict::No)
}

struct Mark {
    uninteresting: bool,
    popped: bool,
    time: i64,
    parents: Vec<Oid>,
}

struct Frontier<'a, S: ?Sized> {
    store: &'a S,
    /// how many commits each side may enqueue, the side to list and the side
    /// that stops it counted apart, so a long `stop_at` history (a target
    /// that moved on since a branch point) never starves the listed one
    cap: usize,
    /// commits enqueued as `[interesting, uninteresting]`
    enqueued: [usize; 2],
    marks: BTreeMap<Oid, Mark>,
    queue: BinaryHeap<(i64, Reverse<Oid>)>,
    pending_interesting: usize,
}

impl<S: Objects + ?Sized> Frontier<'_, S> {
    fn enqueue(&mut self, id: Oid, uninteresting: bool) -> Result<()> {
        let side = &mut self.enqueued[usize::from(uninteresting)];
        if *side >= self.cap {
            return Err(Error::CapReached);
        }
        *side += 1;
        let commit = commit_of(self.store, &id)?;
        self.marks.insert(
            id,
            Mark {
                uninteresting,
                popped: false,
                time: commit.committer.time,
                parents: commit.parents,
            },
        );
        self.queue.push((commit.committer.time, Reverse(id)));
        if !uninteresting {
            self.pending_interesting += 1;
        }
        Ok(())
    }

    fn mark_uninteresting(&mut self, id: Oid) -> Result<()> {
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            let Some(mark) = self.marks.get_mut(&current) else {
                let held_commit = self
                    .store
                    .get(&current)?
                    .is_some_and(|object| object.kind == Kind::Commit);
                if held_commit {
                    self.enqueue(current, true)?;
                }
                continue;
            };
            if mark.uninteresting {
                continue;
            }
            mark.uninteresting = true;
            if mark.popped {
                stack.extend(mark.parents.iter().copied());
            } else {
                self.pending_interesting -= 1;
            }
        }
        Ok(())
    }
}

pub fn commits<S: Objects + ?Sized>(
    store: &S,
    from: &[Oid],
    stop_at: &[Oid],
    cap: usize,
) -> Result<Vec<Oid>> {
    let mut frontier = Frontier {
        store,
        cap,
        enqueued: [0; 2],
        marks: BTreeMap::new(),
        queue: BinaryHeap::new(),
        pending_interesting: 0,
    };
    for stop in stop_at {
        frontier.mark_uninteresting(*stop)?;
    }
    for tip in from {
        let known = frontier.marks.contains_key(tip);
        if !known {
            frontier.enqueue(*tip, false)?;
        }
    }
    while frontier.pending_interesting > 0 {
        let Some((_, Reverse(id))) = frontier.queue.pop() else {
            break;
        };
        let (uninteresting, parents) = {
            let mark = frontier.marks.get_mut(&id).ok_or(Error::Cycle)?;
            mark.popped = true;
            (mark.uninteresting, mark.parents.clone())
        };
        if uninteresting {
            for parent in parents {
                frontier.mark_uninteresting(parent)?;
            }
            continue;
        }
        frontier.pending_interesting -= 1;
        for parent in parents {
            let known = frontier.marks.contains_key(&parent);
            if !known {
                frontier.enqueue(parent, false)?;
            }
        }
    }
    let mut found: Vec<(Reverse<i64>, Oid)> = frontier
        .marks
        .iter()
        .filter(|(_, mark)| mark.popped && !mark.uninteresting)
        .map(|(id, mark)| (Reverse(mark.time), *id))
        .collect();
    found.sort();
    Ok(found.into_iter().map(|(_, id)| id).collect())
}

pub fn newest<S: Objects + ?Sized>(
    store: &S,
    from: &[Oid],
    most: usize,
    cap: usize,
) -> Result<Vec<Oid>> {
    let mut parents: BTreeMap<Oid, Vec<Oid>> = BTreeMap::new();
    let mut queue = BinaryHeap::new();
    for tip in from {
        reach(store, &mut parents, &mut queue, *tip, cap)?;
    }
    let mut listed = Vec::new();
    while listed.len() < most {
        let Some((_, Reverse(id))) = queue.pop() else {
            break;
        };
        listed.push(id);
        if listed.len() == most {
            break;
        }
        let next = parents.get_mut(&id).map(std::mem::take).unwrap_or_default();
        for parent in next {
            reach(store, &mut parents, &mut queue, parent, cap)?;
        }
    }
    Ok(listed)
}

fn reach<S: Objects + ?Sized>(
    store: &S,
    parents: &mut BTreeMap<Oid, Vec<Oid>>,
    queue: &mut BinaryHeap<(i64, Reverse<Oid>)>,
    id: Oid,
    cap: usize,
) -> Result<()> {
    if parents.contains_key(&id) {
        return Ok(());
    }
    if parents.len() >= cap {
        return Err(Error::CapReached);
    }
    let commit = commit_of(store, &id)?;
    queue.push((commit.committer.time, Reverse(id)));
    parents.insert(id, commit.parents);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Walk {
    pub haves: Vec<Oid>,
    pub commits: Vec<Queued>,
    pub objects: Vec<Oid>,
    pub trees: Vec<Pending>,
}

impl Walk {
    pub fn of(wants: &[Oid], haves: &[Oid]) -> Walk {
        Walk {
            haves: haves.to_vec(),
            commits: Vec::new(),
            objects: wants.to_vec(),
            trees: Vec::new(),
        }
    }

    pub fn is_done(&self) -> bool {
        let history_walked = self.commits.iter().all(|queued| queued.uninteresting);
        self.haves.is_empty() && self.objects.is_empty() && self.trees.is_empty() && history_walked
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Queued {
    pub id: Oid,
    pub time: i64,
    pub uninteresting: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub tree: Oid,
    pub against: Vec<Oid>,
    pub from: usize,
}

impl Pending {
    fn of(tree: Oid, against: Vec<Oid>) -> Pending {
        Pending {
            tree,
            against,
            from: 0,
        }
    }

    fn begun(&self) -> bool {
        self.from > 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: Oid,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub listed: Vec<Listed>,
    pub next: Walk,
}

enum Unit {
    Seed(Oid),
    Object(Oid),
    Tree(Pending),
    Commit(i64, Oid),
}

#[derive(Default)]
struct Expansion {
    listed: Vec<Listed>,
    trees: Vec<Pending>,
    rest: Option<Pending>,
}

struct Info {
    time: i64,
    tree: Oid,
    parents: Vec<Oid>,
}

impl Info {
    fn of(commit: &Commit) -> Info {
        Info {
            time: commit.committer.time,
            tree: commit.tree,
            parents: commit.parents.clone(),
        }
    }
}

struct CommitMark {
    time: i64,
    uninteresting: bool,
    popped: bool,
}

struct Page<'a, S: ?Sized> {
    store: &'a S,
    haves: Vec<Oid>,
    objects: VecDeque<Oid>,
    trees: Vec<Pending>,
    marks: BTreeMap<Oid, CommitMark>,
    queue: BinaryHeap<(i64, Reverse<Oid>)>,
    interesting: usize,
    infos: BTreeMap<Oid, Option<Info>>,
    listed: Vec<Listed>,
    seen: BTreeSet<Oid>,
    most: usize,
    expand: bool,
    cut: BTreeSet<Oid>,
}

pub fn closure<S: Objects + ?Sized>(store: &S, walk: Walk, most: usize) -> Result<Closed> {
    walked(Page::open(store, walk, most, true, BTreeSet::new()), 0)
}

// `closure` less the trees: the commits the walk reaches, newest first, and the tags, trees and
// blobs a want names itself. Each commit's tree is left to the client, which reads the tree
// bodies itself and expands them as `closure` would (`Trees`).
pub fn history<S: Objects + ?Sized>(store: &S, walk: Walk, most: usize) -> Result<Closed> {
    walked(Page::open(store, walk, most, false, BTreeSet::new()), 0)
}

// `ran` counts the units run before this page's own (a shallow walk's depths), so a page that made
// progress ends where its budget does rather than failing.
fn walked<S: Objects + ?Sized>(mut page: Page<'_, S>, mut ran: usize) -> Result<Closed> {
    let most = page.most;
    while let Some(unit) = page.next_unit() {
        let page_full = ran > 0 && page.named() >= most;
        if page_full {
            page.put_back(unit);
            break;
        }
        let Err(error) = page.run(&unit) else {
            ran += 1;
            continue;
        };
        let page_full = error == Error::CapReached && ran > 0;
        if !page_full {
            return Err(error);
        }
        page.put_back(unit);
        break;
    }
    Ok(page.close())
}

impl<'a, S: Objects + ?Sized> Page<'a, S> {
    fn open(
        store: &'a S,
        walk: Walk,
        most: usize,
        expand: bool,
        cut: BTreeSet<Oid>,
    ) -> Page<'a, S> {
        let mut page = Page {
            store,
            haves: walk.haves,
            objects: walk.objects.into(),
            trees: walk.trees,
            marks: BTreeMap::new(),
            queue: BinaryHeap::new(),
            interesting: 0,
            infos: BTreeMap::new(),
            listed: Vec::new(),
            seen: BTreeSet::new(),
            most,
            expand,
            cut,
        };
        for queued in walk.commits {
            let known = page.marks.contains_key(&queued.id);
            if !known {
                page.enqueue(queued.id, queued.time, queued.uninteresting);
            }
        }
        page
    }

    fn next_unit(&mut self) -> Option<Unit> {
        if let Some(have) = self.haves.pop() {
            return Some(Unit::Seed(have));
        }
        if let Some(id) = self.objects.pop_front() {
            return Some(Unit::Object(id));
        }
        if let Some(pending) = self.trees.pop() {
            return Some(Unit::Tree(pending));
        }
        if self.interesting == 0 {
            return None;
        }
        let (time, Reverse(id)) = self.queue.pop()?;
        Some(Unit::Commit(time, id))
    }

    fn run(&mut self, unit: &Unit) -> Result<()> {
        match unit {
            Unit::Seed(have) => {
                self.mark_uninteresting(*have, i64::MAX);
                Ok(())
            }
            Unit::Object(id) => self.expand_object(*id),
            Unit::Tree(pending) => self.expand_tree(pending),
            Unit::Commit(time, id) => self.pop_commit(*time, *id),
        }
    }

    fn named(&self) -> usize {
        self.listed.len() + self.trees.len()
    }

    fn put_back(&mut self, unit: Unit) {
        match unit {
            Unit::Seed(have) => self.haves.push(have),
            Unit::Object(id) => self.objects.push_front(id),
            Unit::Tree(pending) => self.trees.push(pending),
            Unit::Commit(time, id) => self.queue.push((time, Reverse(id))),
        }
    }

    fn close(self) -> Closed {
        let history_walked =
            self.interesting == 0 && self.objects.is_empty() && self.haves.is_empty();
        let commits = if history_walked {
            Vec::new()
        } else {
            self.queue
                .into_iter()
                .map(|(time, Reverse(id))| Queued {
                    id,
                    time,
                    uninteresting: self.marks.get(&id).is_some_and(|mark| mark.uninteresting),
                })
                .collect()
        };
        Closed {
            listed: self.listed,
            next: Walk {
                haves: self.haves,
                commits,
                objects: self.objects.into(),
                trees: self.trees,
            },
        }
    }

    fn enqueue(&mut self, id: Oid, time: i64, uninteresting: bool) {
        let mark = CommitMark {
            time,
            uninteresting,
            popped: false,
        };
        self.marks.insert(id, mark);
        self.queue.push((time, Reverse(id)));
        if !uninteresting {
            self.interesting += 1;
        }
    }

    fn mark_uninteresting(&mut self, id: Oid, time: i64) {
        let mut stack = vec![(id, time)];
        while let Some((id, time)) = stack.pop() {
            let Some(mark) = self.marks.get_mut(&id) else {
                self.enqueue(id, time, true);
                continue;
            };
            if mark.uninteresting {
                continue;
            }
            mark.uninteresting = true;
            if !mark.popped {
                self.interesting -= 1;
                continue;
            }
            if let Some(Some(info)) = self.infos.get(&id) {
                stack.extend(info.parents.iter().map(|parent| (*parent, info.time)));
            }
        }
    }

    // A commit as this walk sees it: a cut one (a shallow boundary) has no parents.
    fn info_of(&self, id: &Oid, commit: &Commit) -> Info {
        let mut info = Info::of(commit);
        if self.cut.contains(id) {
            info.parents.clear();
        }
        info
    }

    fn info(&mut self, id: &Oid) -> Result<Option<&Info>> {
        if !self.infos.contains_key(id) {
            let commit = self
                .store
                .get(id)?
                .filter(|object| object.kind == Kind::Commit);
            let info = match commit {
                Some(object) => Some(self.info_of(id, &Commit::parse(&object.body, id.hash())?)),
                None => None,
            };
            self.infos.insert(*id, info);
        }
        Ok(self.infos.get(id).and_then(Option::as_ref))
    }

    fn list(&mut self, id: Oid, kind: Kind) {
        if self.seen.insert(id) {
            self.listed.push(Listed { id, kind });
        }
    }

    fn expand_object(&mut self, id: Oid) -> Result<()> {
        let known = self.marks.contains_key(&id) || self.seen.contains(&id);
        if known {
            return Ok(());
        }
        let object = load(self.store, &id)?;
        match object.kind {
            Kind::Commit => {
                let info = self.info_of(&id, &Commit::parse(&object.body, id.hash())?);
                let time = info.time;
                self.infos.insert(id, Some(info));
                self.enqueue(id, time, false);
            }
            Kind::Tag => {
                let target = Tag::parse(&object.body, id.hash())?.object;
                self.list(id, Kind::Tag);
                self.objects.push_back(target);
            }
            Kind::Tree if self.expand => self.trees.push(Pending::of(id, Vec::new())),
            Kind::Tree => self.list(id, Kind::Tree),
            Kind::Blob => self.list(id, Kind::Blob),
        }
        Ok(())
    }

    fn expand_tree(&mut self, pending: &Pending) -> Result<()> {
        let room = self.most.saturating_sub(self.named()).max(1);
        let expansion = tree_expansion(self.store, pending, room)?;
        for item in expansion.listed {
            self.list(item.id, item.kind);
        }
        self.trees.extend(expansion.rest);
        self.trees.extend(expansion.trees.into_iter().rev());
        Ok(())
    }

    fn pop_commit(&mut self, time: i64, id: Oid) -> Result<()> {
        let uninteresting = self.marks.get(&id).is_some_and(|mark| mark.uninteresting);
        let Some(info) = self.info(&id)? else {
            if !uninteresting {
                return Err(Error::MissingObject(id));
            }
            self.popped(&id);
            return Ok(());
        };
        let (own_time, tree, parents) = (info.time, info.tree, info.parents.clone());
        let early = own_time < time;
        if early {
            if let Some(mark) = self.marks.get_mut(&id) {
                mark.time = own_time;
            }
            self.queue.push((own_time, Reverse(id)));
            return Ok(());
        }
        if uninteresting {
            self.popped(&id);
            for parent in parents {
                self.mark_uninteresting(parent, own_time);
            }
            return Ok(());
        }
        let mut against = Vec::with_capacity(parents.len());
        let mut times = Vec::with_capacity(parents.len());
        for parent in &parents {
            let Some(parent_info) = self.info(parent)? else {
                return Err(Error::MissingObject(*parent));
            };
            against.push(parent_info.tree);
            times.push(parent_info.time);
        }
        self.popped(&id);
        self.interesting -= 1;
        self.list(id, Kind::Commit);
        for (parent, parent_time) in parents.into_iter().zip(times) {
            let known = self.marks.contains_key(&parent);
            if !known {
                self.enqueue(parent, parent_time, false);
            }
        }
        if self.expand {
            self.trees.push(Pending::of(tree, against));
        }
        Ok(())
    }

    fn popped(&mut self, id: &Oid) {
        if let Some(mark) = self.marks.get_mut(id) {
            mark.popped = true;
        }
    }
}

// A shallow fetch's walk (`git fetch --depth`): `history` cut `deepen` commits below each want. It
// first walks the depths, breadth first from the wants (each at depth 1) through every commit
// whatever the client holds, as git's upload-pack does; then `history`, each boundary's parents cut.
// `deepen: None` walks no depths and cuts only at the client's own boundaries (`client_shallows`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Deep {
    pub walk: Walk,
    pub deepen: Option<u32>,
    pub client_shallows: Vec<Oid>,
    pub depths: Vec<(Oid, u32)>,
    pub frontier: Vec<(Oid, u32)>,
}

impl Deep {
    pub fn of(wants: &[Oid], haves: &[Oid], deepen: Option<u32>, client_shallows: &[Oid]) -> Deep {
        let frontier = match deepen {
            Some(_) => wants.iter().map(|want| (*want, 1)).collect(),
            None => Vec::new(),
        };
        Deep {
            walk: Walk::of(wants, haves),
            deepen,
            client_shallows: client_shallows.to_vec(),
            depths: Vec::new(),
            frontier,
        }
    }

    pub fn is_done(&self) -> bool {
        self.frontier.is_empty() && self.walk.is_done()
    }
}

// A page of `shallow_history`. Once the depths are walked, `shallow` names the new boundary (the
// commits at depth `deepen` the client does not hold as boundaries) and `unshallow` the client's
// boundaries nearer than that, whose parents the walk lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deepened {
    pub listed: Vec<Listed>,
    pub next: Deep,
    pub shallow: Vec<Oid>,
    pub unshallow: Vec<Oid>,
}

pub fn shallow_history<S: Objects + ?Sized>(
    store: &S,
    deep: Deep,
    most: usize,
) -> Result<Deepened> {
    let Deep {
        mut walk,
        deepen,
        client_shallows,
        depths,
        frontier,
    } = deep;
    let mut depths = Depths {
        deepen,
        client: client_shallows.iter().copied().collect(),
        known: depths.into_iter().collect(),
        frontier: frontier.into(),
    };
    let mut ran = 0usize;
    while let Some((id, depth)) = depths.frontier.pop_front() {
        let Err(error) = depths.expand(store, &mut walk.objects, id, depth) else {
            ran += 1;
            continue;
        };
        let page_full = error == Error::CapReached && ran > 0;
        if !page_full {
            return Err(error);
        }
        depths.frontier.push_front((id, depth));
        return Ok(depths.page(Vec::new(), walk));
    }
    depths.settle();
    let cut = depths
        .shallow()
        .into_iter()
        .chain(client_shallows)
        .collect();
    let Closed { listed, next } = walked(Page::open(store, walk, most, false, cut), ran)?;
    Ok(depths.page(listed, next))
}

struct Depths {
    deepen: Option<u32>,
    client: BTreeSet<Oid>,
    known: BTreeMap<Oid, u32>,
    frontier: VecDeque<(Oid, u32)>,
}

impl Depths {
    // `id` lies `depth` commits from a want, or nearer: a commit nearer than `deepen` has its
    // parents queued one deeper, and a client boundary among them has its parents wanted.
    fn expand<S: Objects + ?Sized>(
        &mut self,
        store: &S,
        wanted: &mut Vec<Oid>,
        id: Oid,
        depth: u32,
    ) -> Result<()> {
        let Some(deepen) = self.deepen else {
            return Ok(());
        };
        let walked = self.known.get(&id).is_some_and(|known| *known <= depth);
        if walked {
            return Ok(());
        }
        let object = load(store, &id)?;
        match object.kind {
            Kind::Tag => {
                let target = Tag::parse(&object.body, id.hash())?.object;
                self.frontier.push_back((target, depth));
            }
            Kind::Commit => {
                let parents = Commit::parse(&object.body, id.hash())?.parents;
                self.known.insert(id, depth);
                let boundary = depth >= deepen;
                if boundary {
                    return Ok(());
                }
                if self.client.contains(&id) {
                    for parent in &parents {
                        if !wanted.contains(parent) {
                            wanted.push(*parent);
                        }
                    }
                }
                for parent in parents {
                    let nearer = self
                        .known
                        .get(&parent)
                        .is_none_or(|known| *known > depth + 1);
                    if nearer {
                        self.frontier.push_back((parent, depth + 1));
                    }
                }
            }
            Kind::Tree | Kind::Blob => {}
        }
        Ok(())
    }

    fn walked(&self) -> bool {
        self.frontier.is_empty()
    }

    // Once the depths are walked, only the boundaries matter: the commits at `deepen` and the
    // client's own.
    fn settle(&mut self) {
        let (deepen, client) = (self.deepen, &self.client);
        self.known
            .retain(|id, depth| Some(*depth) == deepen || client.contains(id));
    }

    fn shallow(&self) -> Vec<Oid> {
        if !self.walked() {
            return Vec::new();
        }
        self.known
            .iter()
            .filter(|(id, depth)| Some(**depth) == self.deepen && !self.client.contains(*id))
            .map(|(id, _)| *id)
            .collect()
    }

    fn unshallow(&self) -> Vec<Oid> {
        if !self.walked() {
            return Vec::new();
        }
        let nearer = |depth: u32| self.deepen.is_some_and(|deepen| depth < deepen);
        self.known
            .iter()
            .filter(|(id, depth)| self.client.contains(*id) && nearer(**depth))
            .map(|(id, _)| *id)
            .collect()
    }

    fn page(self, listed: Vec<Listed>, walk: Walk) -> Deepened {
        let shallow = self.shallow();
        let unshallow = self.unshallow();
        Deepened {
            listed,
            next: Deep {
                walk,
                deepen: self.deepen,
                client_shallows: self.client.into_iter().collect(),
                depths: self.known.into_iter().collect(),
                frontier: self.frontier.into(),
            },
            shallow,
            unshallow,
        }
    }
}

pub struct Listing {
    walk: Walk,
    received: BTreeSet<Oid>,
    listed: Vec<Listed>,
}

impl Listing {
    pub fn of(wants: &[Oid], haves: &[Oid]) -> Listing {
        Listing {
            walk: Walk::of(wants, haves),
            received: BTreeSet::new(),
            listed: Vec::new(),
        }
    }

    pub fn pending(&mut self) -> Option<Walk> {
        if self.walk.is_done() {
            return None;
        }
        Some(std::mem::take(&mut self.walk))
    }

    pub fn receive(&mut self, closed: Closed) {
        for item in closed.listed {
            if self.received.insert(item.id) {
                self.listed.push(item);
            }
        }
        self.walk = closed.next;
        unreceived(&mut self.walk, &self.received);
    }

    pub fn into_listed(self) -> Vec<Listed> {
        self.listed
    }
}

// A walk sent back less what its earlier pages listed.
fn unreceived(walk: &mut Walk, received: &BTreeSet<Oid>) {
    walk.objects.retain(|id| !received.contains(id));
    walk.commits
        .retain(|queued| queued.uninteresting || !received.contains(&queued.id));
}

// `Listing` for `shallow_history`: what its pages list, once each, and the boundary the last one
// names.
#[derive(Default)]
pub struct ShallowListing {
    deep: Deep,
    received: BTreeSet<Oid>,
    listed: Vec<Listed>,
    shallow: Vec<Oid>,
    unshallow: Vec<Oid>,
}

impl ShallowListing {
    pub fn of(
        wants: &[Oid],
        haves: &[Oid],
        deepen: Option<u32>,
        client_shallows: &[Oid],
    ) -> ShallowListing {
        ShallowListing {
            deep: Deep::of(wants, haves, deepen, client_shallows),
            ..ShallowListing::default()
        }
    }

    pub fn pending(&mut self) -> Option<Deep> {
        if self.deep.is_done() {
            return None;
        }
        Some(std::mem::take(&mut self.deep))
    }

    pub fn receive(&mut self, page: Deepened) {
        for item in page.listed {
            if self.received.insert(item.id) {
                self.listed.push(item);
            }
        }
        self.deep = page.next;
        unreceived(&mut self.deep.walk, &self.received);
        self.shallow = page.shallow;
        self.unshallow = page.unshallow;
    }

    pub fn shallow(&self) -> &[Oid] {
        &self.shallow
    }

    pub fn unshallow(&self) -> &[Oid] {
        &self.unshallow
    }

    pub fn into_listed(self) -> Vec<Listed> {
        self.listed
    }
}

// What `history` leaves to its client: each listed commit's tree compared with its parents'
// trees, and each tree a want names, expanded as `closure` expands them. The client reads the
// trees `wanted` names, then `expand`s one level, until `is_done`.
#[derive(Default)]
pub struct Trees {
    pending: Vec<Pending>,
    expanded: BTreeSet<(Oid, Vec<Oid>)>,
    seen: BTreeSet<Oid>,
    listed: Vec<Listed>,
}

impl Trees {
    pub fn commit(&mut self, tree: Oid, parent_trees: Vec<Oid>) {
        self.pending.push(Pending::of(tree, parent_trees));
    }

    pub fn tree(&mut self, tree: Oid) {
        self.pending.push(Pending::of(tree, Vec::new()));
    }

    pub fn is_done(&self) -> bool {
        self.pending.is_empty()
    }

    // The trees the next `expand` reads: each changed pending tree and those it is compared with.
    pub fn wanted(&self) -> Vec<Oid> {
        let wanted: BTreeSet<Oid> = self
            .pending
            .iter()
            .filter(|pending| !pending.against.contains(&pending.tree))
            .flat_map(|pending| {
                std::iter::once(pending.tree).chain(pending.against.iter().copied())
            })
            .collect();
        wanted.into_iter().collect()
    }

    // One level: each pending tree's entries that no tree it is compared with holds are listed,
    // and its changed subtrees are pending next.
    pub fn expand<S: Objects + ?Sized>(&mut self, store: &S) -> Result<()> {
        for pending in std::mem::take(&mut self.pending) {
            let first = self
                .expanded
                .insert((pending.tree, pending.against.clone()));
            if !first {
                continue;
            }
            let expansion = tree_expansion(store, &pending, usize::MAX)?;
            for item in expansion.listed {
                if self.seen.insert(item.id) {
                    self.listed.push(item);
                }
            }
            self.pending.extend(expansion.trees);
        }
        Ok(())
    }

    pub fn into_listed(self) -> Vec<Listed> {
        self.listed
    }
}

fn tree_expansion<S: Objects + ?Sized>(
    store: &S,
    pending: &Pending,
    room: usize,
) -> Result<Expansion> {
    let unchanged = pending.against.contains(&pending.tree);
    let fresh = !pending.begun();
    if fresh && unchanged {
        return Ok(Expansion::default());
    }
    let tree = tree_of(store, &pending.tree)?;
    let mut against = Vec::with_capacity(pending.against.len());
    for id in &pending.against {
        against.push(tree_of(store, id)?);
    }
    let mut expansion = Expansion::default();
    if fresh {
        expansion.listed.push(Listed {
            id: pending.tree,
            kind: Kind::Tree,
        });
    }
    for (at, entry) in tree.entries.into_iter().enumerate().skip(pending.from) {
        let named = expansion.listed.len() + expansion.trees.len();
        let moved_on = at > pending.from;
        let full = moved_on && named >= room;
        if full {
            expansion.rest = Some(Pending {
                from: at,
                ..pending.clone()
            });
            break;
        }
        let held_by_a_parent = against.iter().any(|parent| {
            parent
                .entries
                .iter()
                .any(|old| old.name == entry.name && old.id == entry.id)
        });
        let skipped = held_by_a_parent || entry.mode == Mode::Gitlink;
        if skipped {
            continue;
        }
        if !entry.mode.is_directory() {
            expansion.listed.push(Listed {
                id: entry.id,
                kind: Kind::Blob,
            });
            continue;
        }
        let against = against
            .iter()
            .filter_map(|parent| {
                parent
                    .entries
                    .iter()
                    .find(|old| old.name == entry.name && old.mode.is_directory())
                    .map(|old| old.id)
            })
            .collect();
        expansion.trees.push(Pending::of(entry.id, against));
    }
    Ok(expansion)
}

pub fn tree_at_path<S: Objects + ?Sized>(
    store: &S,
    tree: &Oid,
    path: &[u8],
) -> Result<Option<TreeEntry>> {
    let mut components = path.split(|byte| *byte == b'/').peekable();
    let mut current = *tree;
    while let Some(component) = components.next() {
        let malformed = component.is_empty();
        if malformed {
            return Ok(None);
        }
        let Some(entry) = tree_of(store, &current)?
            .entries
            .into_iter()
            .find(|e| e.name == component)
        else {
            return Ok(None);
        };
        let last = components.peek().is_none();
        if last {
            return Ok(Some(entry));
        }
        let descends = entry.mode.is_directory();
        if !descends {
            return Ok(None);
        }
        current = entry.id;
    }
    Ok(None)
}

// The merge base: the lowest common ancestor by two-color painting, ties broken by the smaller oid.
const PARENT1: u8 = 1;
const PARENT2: u8 = 2;
const STALE: u8 = 4;
const RESULT: u8 = 8;

struct Paint<'a, S: ?Sized> {
    store: &'a S,
    cap: usize,
    commits: BTreeMap<Oid, (i64, Vec<Oid>)>,
    flags: BTreeMap<Oid, u8>,
    queue: BinaryHeap<(i64, Reverse<Oid>, bool)>,
    fresh_queued: usize,
}

impl<S: Objects + ?Sized> Paint<'_, S> {
    fn info(&mut self, id: Oid) -> Result<(i64, Vec<Oid>)> {
        if let Some(known) = self.commits.get(&id) {
            return Ok(known.clone());
        }
        let over_cap = self.commits.len() >= self.cap;
        if over_cap {
            return Err(Error::CapReached);
        }
        let commit = commit_of(self.store, &id)?;
        let info = (commit.committer.time, commit.parents);
        self.commits.insert(id, info.clone());
        Ok(info)
    }

    fn mark(&mut self, id: Oid, bits: u8) -> Result<()> {
        let existing = self.flags.get(&id).copied().unwrap_or(0);
        let nothing_new = existing & bits == bits;
        if nothing_new {
            return Ok(());
        }
        let updated = existing | bits;
        self.flags.insert(id, updated);
        let (time, _) = self.info(id)?;
        let fresh = updated & STALE == 0;
        if fresh {
            self.fresh_queued += 1;
        }
        self.queue.push((time, Reverse(id), fresh));
        Ok(())
    }
}

pub fn merge_base<S: Objects + ?Sized>(
    store: &S,
    a: &Oid,
    b: &Oid,
    cap: usize,
) -> Result<Option<Oid>> {
    let trivial = a == b;
    if trivial {
        return Ok(Some(*a));
    }
    let mut paint = Paint {
        store,
        cap,
        commits: BTreeMap::new(),
        flags: BTreeMap::new(),
        queue: BinaryHeap::new(),
        fresh_queued: 0,
    };
    paint.mark(*a, PARENT1)?;
    paint.mark(*b, PARENT2)?;
    let mut results = Vec::new();
    while paint.fresh_queued > 0 {
        let Some((_, Reverse(id), was_fresh)) = paint.queue.pop() else {
            break;
        };
        if was_fresh {
            paint.fresh_queued -= 1;
        }
        let flags = paint.flags.get(&id).copied().unwrap_or(0);
        let mut propagate = flags & (PARENT1 | PARENT2 | STALE);
        let reached_from_both = propagate & (PARENT1 | PARENT2) == PARENT1 | PARENT2;
        if reached_from_both {
            let first_time = flags & RESULT == 0;
            if first_time {
                paint.flags.insert(id, flags | RESULT);
                results.push(id);
            }
            propagate |= STALE;
        }
        let (_, parents) = paint.info(id)?;
        for parent in parents {
            paint.mark(parent, propagate)?;
        }
    }
    let mut best: Vec<Oid> = Vec::new();
    for candidate in &results {
        let mut dominated = false;
        for other in &results {
            let same = other == candidate;
            if same {
                continue;
            }
            match is_ancestor(store, candidate, other, cap)? {
                Verdict::Yes => {
                    dominated = true;
                    break;
                }
                Verdict::No => {}
                Verdict::CapReached => return Err(Error::CapReached),
            }
        }
        if !dominated {
            best.push(*candidate);
        }
    }
    Ok(best.into_iter().min())
}
