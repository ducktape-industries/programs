// The 3-way merge a squash takes: trees merged where the sides differ, a file both sides changed merged line by line (diff3), every clash named by its path.

use crate::diff::{Op, myers, split_lines};
use crate::error::{Error, Result};
use crate::object::{Kind, Mode, Object, Tree, TreeEntry};
use crate::oid::{Hash, Oid};
use crate::store::{Objects, load_kind};
use crate::walk::tree_of;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clash {
    // both sides changed the same lines, or lines next to each other
    Lines,
    // the two edits are too interleaved to search within the cost a line merge is held to
    Searched,
    // both sides changed a file that holds a NUL
    Binary,
    // both sides changed its mode, differently
    Mode,
    // one side deleted it and the other changed it
    Deleted,
    // changed on both sides, and not as two files: a directory against a file, a symlink, a gitlink
    Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub path: Vec<u8>,
    pub clash: Clash,
}

// The merged root tree and the objects it adds; it stands only when `conflicts` is empty.
#[derive(Clone, Debug)]
pub struct Merge {
    pub tree: Oid,
    pub objects: BTreeMap<Oid, Object>,
    pub conflicts: Vec<Conflict>,
}

// Merges `theirs` onto `ours` from `base` (none: the empty tree). Only trees that differ on
// both sides are read; every conflict over the whole walk is gathered.
pub fn trees<S: Objects + ?Sized>(
    store: &S,
    base: Option<&Oid>,
    ours: &Oid,
    theirs: &Oid,
    max_cost: usize,
) -> Result<Merge> {
    let mut merging = Merging {
        store,
        hash: ours.hash(),
        max_cost,
        objects: BTreeMap::new(),
        conflicts: Vec::new(),
    };
    let root = merging.tree(&mut Vec::new(), base.copied(), Some(*ours), Some(*theirs))?;
    let tree = match root {
        Some(tree) => tree,
        None => merging.keep(Kind::Tree, Vec::new())?,
    };
    Ok(Merge {
        tree,
        objects: merging.objects,
        conflicts: merging.conflicts,
    })
}

type Entry = (Mode, Oid);

struct Merging<'a, S: ?Sized> {
    store: &'a S,
    hash: Hash,
    max_cost: usize,
    objects: BTreeMap<Oid, Object>,
    conflicts: Vec<Conflict>,
}

impl<S: Objects + ?Sized> Merging<'_, S> {
    fn keep(&mut self, kind: Kind, body: Vec<u8>) -> Result<Oid> {
        let object = Object::new(kind, body);
        let id = object.id(self.hash)?;
        self.objects.insert(id, object);
        Ok(id)
    }

    fn clash(&mut self, path: &[u8], clash: Clash) {
        self.conflicts.push(Conflict {
            path: path.to_vec(),
            clash,
        });
    }

    fn entries(&self, tree: Option<Oid>) -> Result<BTreeMap<Vec<u8>, Entry>> {
        let Some(id) = tree else {
            return Ok(BTreeMap::new());
        };
        let entries = tree_of(self.store, &id)?.entries;
        Ok(entries
            .into_iter()
            .map(|entry| (entry.name, (entry.mode, entry.id)))
            .collect())
    }

    fn tree(
        &mut self,
        path: &mut Vec<u8>,
        base: Option<Oid>,
        ours: Option<Oid>,
        theirs: Option<Oid>,
    ) -> Result<Option<Oid>> {
        if theirs == base {
            return Ok(ours);
        }
        if ours == base || ours == theirs {
            return Ok(theirs);
        }
        let base = self.entries(base)?;
        let ours = self.entries(ours)?;
        let theirs = self.entries(theirs)?;
        let names: BTreeSet<&Vec<u8>> = base
            .keys()
            .chain(ours.keys())
            .chain(theirs.keys())
            .collect();
        let mut entries = Vec::new();
        for name in names {
            let at = path.len();
            if at > 0 {
                path.push(b'/');
            }
            path.extend_from_slice(name);
            let sides = [base.get(name), ours.get(name), theirs.get(name)].map(|e| e.copied());
            let merged = self.entry(path, sides)?;
            path.truncate(at);
            if let Some((mode, id)) = merged {
                entries.push(TreeEntry {
                    mode,
                    name: name.clone(),
                    id,
                });
            }
        }
        if entries.is_empty() {
            return Ok(None);
        }
        let body = Tree { entries }.serialize();
        self.keep(Kind::Tree, body).map(Some)
    }

    fn entry(
        &mut self,
        path: &mut Vec<u8>,
        [base, ours, theirs]: [Option<Entry>; 3],
    ) -> Result<Option<Entry>> {
        if theirs == base {
            return Ok(ours);
        }
        if ours == base || ours == theirs {
            return Ok(theirs);
        }
        let (Some(our), Some(their)) = (ours, theirs) else {
            self.clash(path, Clash::Deleted);
            return Ok(ours);
        };
        if our.0.is_directory() && their.0.is_directory() {
            let base = base
                .filter(|(mode, _)| mode.is_directory())
                .map(|(_, id)| id);
            let tree = self.tree(path, base, Some(our.1), Some(their.1))?;
            return Ok(tree.map(|id| (Mode::Directory, id)));
        }
        let files =
            our.0.is_file() && their.0.is_file() && base.is_none_or(|(mode, _)| mode.is_file());
        if !files {
            self.clash(path, Clash::Kind);
            return Ok(ours);
        }
        self.file(path, base, our, their)
    }

    fn file(
        &mut self,
        path: &[u8],
        base: Option<Entry>,
        ours: Entry,
        theirs: Entry,
    ) -> Result<Option<Entry>> {
        let Some(mode) = settle_one_sided_change(base.map(|(mode, _)| mode), ours.0, theirs.0)
        else {
            self.clash(path, Clash::Mode);
            return Ok(Some(ours));
        };
        let base = base.map(|(_, id)| id);
        let id = match settle_one_sided_change(base, ours.1, theirs.1) {
            Some(id) => id,
            None => match self.lines(path, base, ours.1, theirs.1)? {
                Some(id) => id,
                None => return Ok(Some(ours)),
            },
        };
        Ok(Some((mode, id)))
    }

    fn lines(
        &mut self,
        path: &[u8],
        base: Option<Oid>,
        ours: Oid,
        theirs: Oid,
    ) -> Result<Option<Oid>> {
        let base = match base {
            Some(id) => self.blob(&id)?,
            None => Vec::new(),
        };
        let (ours, theirs) = (self.blob(&ours)?, self.blob(&theirs)?);
        if [&base, &ours, &theirs]
            .iter()
            .any(|bytes| bytes.contains(&0))
        {
            self.clash(path, Clash::Binary);
            return Ok(None);
        }
        match diff3(&base, &ours, &theirs, self.max_cost) {
            Ok(Some(merged)) => self.keep(Kind::Blob, merged).map(Some),
            Ok(None) => {
                self.clash(path, Clash::Lines);
                Ok(None)
            }
            Err(Error::CapReached) => {
                self.clash(path, Clash::Searched);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    fn blob(&self, id: &Oid) -> Result<Vec<u8>> {
        Ok(load_kind(self.store, id, Kind::Blob)?.body)
    }
}

fn settle_one_sided_change<T: PartialEq + Copy>(base: Option<T>, ours: T, theirs: T) -> Option<T> {
    if ours == theirs || base == Some(theirs) {
        return Some(ours);
    }
    if base == Some(ours) {
        return Some(theirs);
    }
    None
}

struct Edit {
    base: Range<usize>,
    side: Range<usize>,
}

fn edits(ops: &[Op]) -> Vec<Edit> {
    let mut edits: Vec<Edit> = Vec::new();
    let (mut base, mut side, mut open) = (0, 0, false);
    for op in ops {
        if *op == Op::Equal {
            (base, side, open) = (base + 1, side + 1, false);
            continue;
        }
        if !open {
            edits.push(Edit {
                base: base..base,
                side: side..side,
            });
            open = true;
        }
        match op {
            Op::Delete => base += 1,
            _ => side += 1,
        }
        let edit = edits.last_mut().expect("an edit is open");
        (edit.base.end, edit.side.end) = (base, side);
    }
    edits
}

fn side_lines_for_base_span(
    edits: &[Edit],
    lines: &[&[u8]],
    base: &[&[u8]],
    span: &Range<usize>,
) -> Vec<u8> {
    let (Some(first), Some(last)) = (edits.first(), edits.last()) else {
        return base[span.clone()].concat();
    };
    let start = first.side.start - (first.base.start - span.start);
    let end = last.side.end + (span.end - last.base.end);
    lines[start..end].concat()
}

// A 3-way line merge, lines split at `\n` as `diff::split_lines` splits them. Each side's edits
// to `base` (Myers, held to `max_cost`) are taken where they stand apart; edits of the two sides
// that overlap or touch (adjacent lines, two insertions at one point) are one group, which
// merges only when both sides wrote the same lines. `None` on a conflict.
pub fn diff3(base: &[u8], ours: &[u8], theirs: &[u8], max_cost: usize) -> Result<Option<Vec<u8>>> {
    let base = split_lines(base);
    let our_lines = split_lines(ours);
    let their_lines = split_lines(theirs);
    let our_edits = edits(&myers(&base, &our_lines, max_cost)?);
    let their_edits = edits(&myers(&base, &their_lines, max_cost)?);
    let mut merged = Vec::new();
    let (mut copied, mut i, mut j) = (0, 0, 0);
    loop {
        let start = match (our_edits.get(i), their_edits.get(j)) {
            (None, None) => break,
            (Some(edit), None) | (None, Some(edit)) => edit.base.start,
            (Some(ours), Some(theirs)) => ours.base.start.min(theirs.base.start),
        };
        let (mut end, mut oi, mut tj) = (start, i, j);
        loop {
            let touches = |edit: &&Edit| edit.base.start <= end;
            if let Some(edit) = our_edits.get(oi).filter(touches) {
                end = end.max(edit.base.end);
                oi += 1;
            } else if let Some(edit) = their_edits.get(tj).filter(touches) {
                end = end.max(edit.base.end);
                tj += 1;
            } else {
                break;
            }
        }
        let span = start..end;
        let ours = side_lines_for_base_span(&our_edits[i..oi], &our_lines, &base, &span);
        let theirs = side_lines_for_base_span(&their_edits[j..tj], &their_lines, &base, &span);
        let text = match (oi > i, tj > j) {
            (true, false) => ours,
            (false, true) => theirs,
            _ if ours == theirs => ours,
            _ => return Ok(None),
        };
        merged.extend(base[copied..start].concat());
        merged.extend(text);
        (copied, i, j) = (end, oi, tj);
    }
    merged.extend(base[copied..].concat());
    Ok(Some(merged))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryObjects;

    fn merged(base: &str, ours: &str, theirs: &str) -> Option<String> {
        let merged = diff3(base.as_bytes(), ours.as_bytes(), theirs.as_bytes(), 100).unwrap();
        merged.map(|bytes| String::from_utf8(bytes).unwrap())
    }

    #[test]
    fn edits_apart_merge_and_edits_that_touch_conflict() {
        let base = "1\n2\n3\n4\n5\n";
        let apart = merged(base, "one\n2\n3\n4\n5\n", "1\n2\n3\n4\nfive\n");
        assert_eq!(apart.as_deref(), Some("one\n2\n3\n4\nfive\n"));
        let same = merged(base, "1\ntwo\n3\n4\n5\n", "1\ntwo\n3\n4\n5\n");
        assert_eq!(same.as_deref(), Some("1\ntwo\n3\n4\n5\n"));
        let deleted = merged(base, "2\n3\n4\n5\n", "1\n2\n3\nfour\n5\n");
        assert_eq!(deleted.as_deref(), Some("2\n3\nfour\n5\n"));
        let one_side = merged(base, base, "1\n2\nthree\n4\n5\n");
        assert_eq!(one_side.as_deref(), Some("1\n2\nthree\n4\n5\n"));
        assert_eq!(
            merged(base, "1\nII\n3\n4\n5\n", "1\n2\n3\n4\n5\n6\n").as_deref(),
            Some("1\nII\n3\n4\n5\n6\n")
        );
        assert_eq!(
            merged(base, "1\ntwo\n3\n4\n5\n", "1\n2\n3\n4\nV\n").as_deref(),
            Some("1\ntwo\n3\n4\nV\n")
        );
        assert_eq!(merged(base, "1\nA\n3\n4\n5\n", "1\nB\n3\n4\n5\n"), None);
        assert_eq!(
            merged(base, "1\ntwo\n3\n4\n5\n", "1\n2\nthree\n4\n5\n"),
            None
        );
    }

    #[test]
    fn insertions_at_one_point_conflict_unless_they_agree() {
        let base = "1\n2\n3\n";
        assert_eq!(merged(base, "1\n2\na\n3\n", "1\n2\nb\n3\n"), None);
        let agreed = merged(base, "1\n2\na\n3\n", "1\n2\na\n3\n");
        assert_eq!(agreed.as_deref(), Some("1\n2\na\n3\n"));
        let apart = merged(base, "0\n1\n2\n3\n", "1\n2\n3\n4\n");
        assert_eq!(apart.as_deref(), Some("0\n1\n2\n3\n4\n"));
        assert_eq!(merged("", "a\n", "b\n"), None);
        assert_eq!(merged("", "a\n", "a\n").as_deref(), Some("a\n"));
    }

    #[test]
    fn a_missing_final_newline_is_kept() {
        let kept = merged("a\nb\nc", "A\nb\nc", "a\nb\nC");
        assert_eq!(kept.as_deref(), Some("A\nb\nC"));
    }

    #[test]
    fn a_search_past_its_cost_is_refused() {
        let refused = diff3(b"a\nb\nc\n", b"x\nb\ny\n", b"a\nb\nc\n", 0);
        assert_eq!(refused, Err(Error::CapReached));
    }

    fn files(store: &mut MemoryObjects, listed: &[(&str, &str)]) -> Oid {
        let mut entries = Vec::new();
        let mut dirs: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for (path, text) in listed {
            match path.split_once('/') {
                Some((dir, rest)) => dirs.entry(dir).or_default().push((rest, *text)),
                None => entries.push(TreeEntry {
                    mode: Mode::Regular,
                    name: path.as_bytes().to_vec(),
                    id: store.put(Kind::Blob, text.as_bytes()).unwrap(),
                }),
            }
        }
        for (dir, inside) in dirs {
            entries.push(TreeEntry {
                mode: Mode::Directory,
                name: dir.as_bytes().to_vec(),
                id: files(store, &inside),
            });
        }
        store
            .put(Kind::Tree, &Tree { entries }.serialize())
            .unwrap()
    }

    type Files<'a> = &'a [(&'a str, &'a str)];

    fn three(store: &mut MemoryObjects, base: Files, ours: Files, theirs: Files) -> Merge {
        let (base, ours, theirs) = (files(store, base), files(store, ours), files(store, theirs));
        trees(&*store, Some(&base), &ours, &theirs, 100).unwrap()
    }

    #[test]
    fn each_side_keeps_what_only_it_changed() {
        let mut store = MemoryObjects::new(Hash::Sha1);
        let base = [("a", "1\n"), ("b", "1\n"), ("d/x", "1\n2\n3\n")];
        let ours = [("a", "2\n"), ("b", "1\n"), ("d/x", "one\n2\n3\n")];
        let theirs = [("a", "1\n"), ("c", "3\n"), ("d/x", "1\n2\nthree\n")];
        let merge = three(&mut store, &base, &ours, &theirs);
        assert!(merge.conflicts.is_empty());
        let expected = files(
            &mut store,
            &[("a", "2\n"), ("c", "3\n"), ("d/x", "one\n2\nthree\n")],
        );
        assert_eq!(merge.tree, expected);
        let again = three(&mut store, &base, &ours, &theirs);
        assert_eq!(again.tree, merge.tree);
        assert_eq!(again.objects, merge.objects);
    }

    #[test]
    fn every_clash_is_named_by_its_path() {
        let mut store = MemoryObjects::new(Hash::Sha1);
        let base = [("a", "1\n"), ("b", "1\n"), ("c", "1\n"), ("d/z", "\0a")];
        let ours = [("a", "2\n"), ("c", "2\n"), ("d/z", "\0b")];
        let theirs = [("a", "3\n"), ("b", "2\n"), ("c/x", "1\n"), ("d/z", "\0c")];
        let merge = three(&mut store, &base, &ours, &theirs);
        let clash = |path: &str, clash| Conflict {
            path: path.as_bytes().to_vec(),
            clash,
        };
        assert_eq!(
            merge.conflicts,
            [
                clash("a", Clash::Lines),
                clash("b", Clash::Deleted),
                clash("c", Clash::Kind),
                clash("d/z", Clash::Binary)
            ]
        );
    }

    #[test]
    fn a_directory_both_sides_emptied_is_dropped() {
        let mut store = MemoryObjects::new(Hash::Sha1);
        let base = [("d/x", "x"), ("d/y", "y")];
        let merge = three(&mut store, &base, &[("d/x", "x")], &[("d/y", "y")]);
        assert!(merge.conflicts.is_empty());
        assert_eq!(merge.tree, files(&mut store, &[]));
    }
}
