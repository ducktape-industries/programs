// Tree diff between two trees in the store, Myers line diff, and unified-diff text.

use crate::error::{Error, Result};
use crate::object::{Mode, TreeEntry};
use crate::oid::{Oid, push_decimal};
use crate::store::Objects;
use crate::walk::tree_of;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added {
        mode: Mode,
        id: Oid,
    },
    Deleted {
        mode: Mode,
        id: Oid,
    },
    Modified {
        old_mode: Mode,
        new_mode: Mode,
        old: Oid,
        new: Oid,
    },
    ModeChanged {
        old_mode: Mode,
        new_mode: Mode,
        id: Oid,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: Vec<u8>,
    pub kind: ChangeKind,
}

pub fn tree_diff<S: Objects + ?Sized>(
    store: &S,
    old: Option<&Oid>,
    new: Option<&Oid>,
) -> Result<Vec<Change>> {
    let mut out = Vec::new();
    diff_trees(store, old.copied(), new.copied(), &mut Vec::new(), &mut out)?;
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn entries_of<S: Objects + ?Sized>(
    store: &S,
    tree: Option<Oid>,
) -> Result<BTreeMap<Vec<u8>, TreeEntry>> {
    let Some(id) = tree else {
        return Ok(BTreeMap::new());
    };
    Ok(tree_of(store, &id)?
        .entries
        .into_iter()
        .map(|entry| (entry.name.clone(), entry))
        .collect())
}

fn diff_trees<S: Objects + ?Sized>(
    store: &S,
    old: Option<Oid>,
    new: Option<Oid>,
    prefix: &mut Vec<u8>,
    out: &mut Vec<Change>,
) -> Result<()> {
    let identical = old.is_some() && old == new;
    if identical {
        return Ok(());
    }
    let old_entries = entries_of(store, old)?;
    let new_entries = entries_of(store, new)?;
    let names: BTreeSet<&Vec<u8>> = old_entries.keys().chain(new_entries.keys()).collect();
    for name in names {
        let base_len = prefix.len();
        if !prefix.is_empty() {
            prefix.push(b'/');
        }
        prefix.extend_from_slice(name);
        match (old_entries.get(name), new_entries.get(name)) {
            (None, Some(added)) => add_entry(store, added, prefix, out)?,
            (Some(deleted), None) => delete_entry(store, deleted, prefix, out)?,
            (Some(before), Some(after)) => diff_entry(store, before, after, prefix, out)?,
            (None, None) => {}
        }
        prefix.truncate(base_len);
    }
    Ok(())
}

fn add_entry<S: Objects + ?Sized>(
    store: &S,
    entry: &TreeEntry,
    prefix: &mut Vec<u8>,
    out: &mut Vec<Change>,
) -> Result<()> {
    if entry.mode.is_directory() {
        return diff_trees(store, None, Some(entry.id), prefix, out);
    }
    out.push(Change {
        path: prefix.clone(),
        kind: ChangeKind::Added {
            mode: entry.mode,
            id: entry.id,
        },
    });
    Ok(())
}

fn delete_entry<S: Objects + ?Sized>(
    store: &S,
    entry: &TreeEntry,
    prefix: &mut Vec<u8>,
    out: &mut Vec<Change>,
) -> Result<()> {
    if entry.mode.is_directory() {
        return diff_trees(store, Some(entry.id), None, prefix, out);
    }
    out.push(Change {
        path: prefix.clone(),
        kind: ChangeKind::Deleted {
            mode: entry.mode,
            id: entry.id,
        },
    });
    Ok(())
}

fn diff_entry<S: Objects + ?Sized>(
    store: &S,
    before: &TreeEntry,
    after: &TreeEntry,
    prefix: &mut Vec<u8>,
    out: &mut Vec<Change>,
) -> Result<()> {
    let unchanged = before.id == after.id && before.mode == after.mode;
    if unchanged {
        return Ok(());
    }
    match (before.mode.is_directory(), after.mode.is_directory()) {
        (true, true) => diff_trees(store, Some(before.id), Some(after.id), prefix, out),
        (true, false) => {
            delete_entry(store, before, prefix, out)?;
            add_entry(store, after, prefix, out)
        }
        (false, true) => {
            delete_entry(store, before, prefix, out)?;
            add_entry(store, after, prefix, out)
        }
        (false, false) => {
            let same_content = before.id == after.id;
            let kind = if same_content {
                ChangeKind::ModeChanged {
                    old_mode: before.mode,
                    new_mode: after.mode,
                    id: before.id,
                }
            } else {
                ChangeKind::Modified {
                    old_mode: before.mode,
                    new_mode: after.mode,
                    old: before.id,
                    new: after.id,
                }
            };
            out.push(Change {
                path: prefix.clone(),
                kind,
            });
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Equal,
    Insert,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    pub op: Op,
    pub old: Range<usize>,
    pub new: Range<usize>,
}

pub fn split_lines(text: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(rest.len(), |i| i + 1);
        lines.push(&rest[..end]);
        rest = &rest[end..];
    }
    lines
}

pub fn lines(old: &[u8], new: &[u8], max_cost: usize) -> Result<Vec<Hunk>> {
    let a = split_lines(old);
    let b = split_lines(new);
    let ops = myers(&a, &b, max_cost)?;
    Ok(coalesce(&ops))
}

pub(crate) fn myers<T: PartialEq>(a: &[T], b: &[T], max_cost: usize) -> Result<Vec<Op>> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[head..], &b[head..]);
    let tail = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - tail], &b[..b.len() - tail]);
    let mut ops = vec![Op::Equal; head];
    if a.is_empty() || b.is_empty() {
        ops.extend(std::iter::repeat_n(Op::Delete, a.len()));
        ops.extend(std::iter::repeat_n(Op::Insert, b.len()));
    } else {
        ops.extend(search(a, b, max_cost)?);
    }
    ops.extend(std::iter::repeat_n(Op::Equal, tail));
    Ok(ops)
}

fn search<T: PartialEq>(a: &[T], b: &[T], max_cost: usize) -> Result<Vec<Op>> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    let max = n + m;
    let offset = (max + 1) as usize;
    let mut v = vec![0isize; 2 * offset + 1];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max {
        let over_cost = d as usize > max_cost;
        if over_cost {
            return Err(Error::CapReached);
        }
        let low = (offset as isize - d) as usize;
        let high = (offset as isize + d) as usize;
        trace.push(v[low..=high].to_vec());
        let mut k = -d;
        while k <= d {
            let index = (k + offset as isize) as usize;
            let down = k == -d || (k != d && v[index - 1] < v[index + 1]);
            let mut x = if down { v[index + 1] } else { v[index - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[index] = x;
            let reached_end = x >= n && y >= m;
            if reached_end {
                return Ok(backtrack(&trace, n, m));
            }
            k += 2;
        }
    }
    Err(Error::CapReached)
}

fn backtrack(trace: &[Vec<isize>], n: isize, m: isize) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut x = n;
    let mut y = m;
    for (d, v) in trace.iter().enumerate().rev() {
        let d = d as isize;
        let at = |k: isize| v[(k + d) as usize];
        let k = x - y;
        let start = d == 0;
        if start {
            while x > 0 && y > 0 {
                ops.push(Op::Equal);
                x -= 1;
                y -= 1;
            }
            break;
        }
        let down = k == -d || (k != d && at(k - 1) < at(k + 1));
        let prev_k = if down { k + 1 } else { k - 1 };
        let prev_x = at(prev_k);
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y {
            ops.push(Op::Equal);
            x -= 1;
            y -= 1;
        }
        ops.push(if down { Op::Insert } else { Op::Delete });
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    ops
}

fn coalesce(ops: &[Op]) -> Vec<Hunk> {
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut old = 0;
    let mut new = 0;
    for op in ops {
        let (old_step, new_step) = match op {
            Op::Equal => (1, 1),
            Op::Insert => (0, 1),
            Op::Delete => (1, 0),
        };
        let extends_last = hunks.last().is_some_and(|last| last.op == *op);
        if extends_last {
            let last = hunks.last_mut().expect("checked above");
            last.old.end += old_step;
            last.new.end += new_step;
        } else {
            hunks.push(Hunk {
                op: *op,
                old: old..old + old_step,
                new: new..new + new_step,
            });
        }
        old += old_step;
        new += new_step;
    }
    hunks
}

struct LineOp {
    op: Op,
    old_index: usize,
    new_index: usize,
}

fn line_ops(ops: &[Op]) -> Vec<LineOp> {
    let mut old_index = 0;
    let mut new_index = 0;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(LineOp {
            op: *op,
            old_index,
            new_index,
        });
        match op {
            Op::Equal => {
                old_index += 1;
                new_index += 1;
            }
            Op::Insert => new_index += 1,
            Op::Delete => old_index += 1,
        }
    }
    out
}

pub fn unified(old: &[u8], new: &[u8], context: usize, max_cost: usize) -> Result<Vec<u8>> {
    let a = split_lines(old);
    let b = split_lines(new);
    let ops = line_ops(&myers(&a, &b, max_cost)?);
    let changes: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| op.op != Op::Equal)
        .map(|(index, _)| index)
        .collect();
    let mut out = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for change in changes {
        let joins_last = groups
            .last()
            .is_some_and(|(_, last)| change - last - 1 <= 2 * context);
        if joins_last {
            groups.last_mut().expect("checked above").1 = change;
        } else {
            groups.push((change, change));
        }
    }
    for (first, last) in groups {
        let start = first.saturating_sub(context);
        let end = (last + context + 1).min(ops.len());
        let window = &ops[start..end];
        let old_count = window.iter().filter(|op| op.op != Op::Insert).count();
        let new_count = window.iter().filter(|op| op.op != Op::Delete).count();
        out.extend_from_slice(b"@@ -");
        push_range(&mut out, window[0].old_index, old_count);
        out.extend_from_slice(b" +");
        push_range(&mut out, window[0].new_index, new_count);
        out.extend_from_slice(b" @@\n");
        for op in window {
            let (marker, line) = match op.op {
                Op::Equal => (b' ', a[op.old_index]),
                Op::Delete => (b'-', a[op.old_index]),
                Op::Insert => (b'+', b[op.new_index]),
            };
            out.push(marker);
            out.extend_from_slice(line);
            let unterminated = !line.ends_with(b"\n");
            if unterminated {
                out.extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
    }
    Ok(out)
}

fn push_range(out: &mut Vec<u8>, start: usize, count: usize) {
    match count {
        0 => {
            push_decimal(out, start as u64);
            out.extend_from_slice(b",0");
        }
        1 => push_decimal(out, start as u64 + 1),
        _ => {
            push_decimal(out, start as u64 + 1);
            out.push(b',');
            push_decimal(out, count as u64);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn myers_edit_scripts() {
        assert_eq!(myers(b"abc", b"abc", 10).unwrap(), [Op::Equal; 3]);
        assert_eq!(
            myers(b"abcabba", b"cbabac", 10)
                .unwrap()
                .iter()
                .filter(|op| **op != Op::Equal)
                .count(),
            5
        );
        assert_eq!(myers(b"", b"ab", 10).unwrap(), [Op::Insert, Op::Insert]);
        assert_eq!(myers(b"ab", b"", 10).unwrap(), [Op::Delete, Op::Delete]);
        assert_eq!(myers::<u8>(b"", b"", 10).unwrap(), Vec::<Op>::new());
        assert_eq!(myers(b"abcd", b"xbcy", 1), Err(Error::CapReached));
    }

    #[test]
    fn one_sided_edits_are_not_searched() {
        let long: Vec<u8> = (0..5000u32).map(|i| i as u8).collect();
        assert_eq!(myers(b"", &long, 0).unwrap(), vec![Op::Insert; 5000]);
        assert_eq!(myers(&long, b"", 0).unwrap(), vec![Op::Delete; 5000]);
        let mut grown = b"ab".to_vec();
        grown.extend_from_slice(&long);
        grown.extend_from_slice(b"cd");
        let mut expected = vec![Op::Equal; 2];
        expected.extend(vec![Op::Insert; 5000]);
        expected.extend(vec![Op::Equal; 2]);
        assert_eq!(myers(b"abcd", &grown, 0).unwrap(), expected);
        assert_eq!(
            myers(b"abXcdYef", b"abPcdQef", 10).unwrap(),
            [
                Op::Equal,
                Op::Equal,
                Op::Delete,
                Op::Insert,
                Op::Equal,
                Op::Equal,
                Op::Delete,
                Op::Insert,
                Op::Equal,
                Op::Equal
            ]
        );
        assert_eq!(myers(b"abXcdYef", b"abPcdQef", 3), Err(Error::CapReached));
    }

    #[test]
    fn trimming_keeps_the_plain_search_answer() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move |bound: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % bound
        };
        let edits = |ops: &[Op]| ops.iter().filter(|op| **op != Op::Equal).count();
        for _ in 0..2000 {
            let mut text = || -> Vec<u8> { (0..next(12)).map(|_| b'a' + next(3) as u8).collect() };
            let (a, b) = (text(), text());
            let cost = next(14) as usize;
            let plain = search(&a, &b, cost);
            let trimmed = myers(&a, &b, cost);
            let Ok(ops) = trimmed else {
                assert!(plain.is_err(), "{a:?} {b:?} {cost}");
                continue;
            };
            let (mut i, mut j) = (0, 0);
            for op in &ops {
                match op {
                    Op::Equal => {
                        assert_eq!(a[i], b[j]);
                        (i, j) = (i + 1, j + 1);
                    }
                    Op::Insert => j += 1,
                    Op::Delete => i += 1,
                }
            }
            assert_eq!((i, j), (a.len(), b.len()));
            match plain {
                Ok(plain) => assert_eq!(edits(&ops), edits(&plain), "{a:?} {b:?}"),
                Err(_) => assert!(edits(&ops) > cost, "{a:?} {b:?} {cost}"),
            }
        }
    }

    #[test]
    fn edit_script_replays_old_into_new() {
        let old = b"the quick brown fox jumps";
        let new = b"a quick red fox leaps high";
        let ops = myers(old, new, 100).unwrap();
        let mut rebuilt = Vec::new();
        let mut i = 0;
        let mut j = 0;
        for op in ops {
            match op {
                Op::Equal => {
                    assert_eq!(old[i], new[j]);
                    rebuilt.push(old[i]);
                    i += 1;
                    j += 1;
                }
                Op::Insert => {
                    rebuilt.push(new[j]);
                    j += 1;
                }
                Op::Delete => i += 1,
            }
        }
        assert_eq!(rebuilt, new);
    }

    #[test]
    fn split_keeps_newlines_and_last_partial_line() {
        assert_eq!(split_lines(b"a\nb\nc"), [&b"a\n"[..], b"b\n", b"c"]);
        assert_eq!(split_lines(b"a\n"), [&b"a\n"[..]]);
        assert!(split_lines(b"").is_empty());
    }
}
