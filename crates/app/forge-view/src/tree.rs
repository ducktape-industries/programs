//! The Code tab's file tree: a directory's children open inline beneath
//! it. Each expanded directory is its own lazy `Query::Tree`; the rows on
//! screen are a walk of whatever of those has landed.
use view::prelude::*;

use crate::queries::PAGE;
use crate::state::Forge;
use crate::{Stage, ui::code::join};
use forge::{EntryKind, Query, Reply, TreeInfo};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Entry { kind: EntryKind, oid: String },
    Loading,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub path: Vec<u8>,
    pub name: String,
    pub depth: usize,
    pub slot: Slot,
}

impl Row {
    pub fn is_dir(&self) -> bool {
        matches!(
            self.slot,
            Slot::Entry {
                kind: EntryKind::Directory,
                ..
            }
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Enter,
}

impl Key {
    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "enter" => Self::Enter,
            _ => return None,
        })
    }
}

impl Forge {
    pub(crate) fn tree_query(&self, path: Vec<u8>) -> Option<Query> {
        Some(Query::Tree {
            repo: self.nav.repo.clone()?,
            at: self.head_oid()?,
            path,
            page: PAGE,
        })
    }

    pub(crate) fn tree_queries(&self) -> Vec<Query> {
        std::iter::once(Vec::new())
            .chain(self.nav.expanded.iter().cloned())
            .chain(self.nav.folder.clone())
            .filter_map(|path| self.tree_query(path))
            .collect()
    }

    pub(crate) fn tree_rows(&self) -> Vec<Row> {
        self.tree_rows_at(Vec::new())
    }

    pub(crate) fn tree_rows_at(&self, dir: Vec<u8>) -> Vec<Row> {
        let mut rows = Vec::new();
        let needle = self.tree_search.text().trim().to_lowercase();
        self.walk(dir, 0, &needle, &mut rows);
        rows
    }

    fn walk(&self, dir: Vec<u8>, depth: usize, needle: &str, rows: &mut Vec<Row>) {
        let Some(query) = self.tree_query(dir.clone()) else {
            return;
        };
        let mut items: Vec<&TreeInfo> = match self.stage(&query) {
            Stage::Ready(Reply::Tree { page, .. }) => page.items.iter().collect(),
            _ if depth == 0 => return,
            Stage::Loading => return rows.push(placeholder(&dir, depth, Slot::Loading)),
            Stage::Failed(refusal) => {
                let failed = Slot::Failed(design::sentence(refusal));
                return rows.push(placeholder(&dir, depth, failed));
            }
            Stage::Ready(_) => return,
        };
        items.sort_by_key(|entry| (entry.kind != EntryKind::Directory, entry.name.clone()));
        for entry in items {
            let name = String::from_utf8_lossy(&entry.name).into_owned();
            let is_dir = entry.kind == EntryKind::Directory;
            if !is_dir && !needle.is_empty() && !name.to_lowercase().contains(needle) {
                continue;
            }
            let path = join(&dir, &entry.name);
            rows.push(Row {
                path: path.clone(),
                name,
                depth,
                slot: Slot::Entry {
                    kind: entry.kind,
                    oid: entry.oid.clone(),
                },
            });
            if is_dir && self.nav.expanded.contains(&path) {
                self.walk(path, depth + 1, needle, rows);
            }
        }
    }

    pub(crate) fn toggle_dir(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        if !self.nav.expanded.remove(&path) {
            self.nav.expanded.insert(path.clone());
        }
        self.nav.cursor = Some(path);
        cx.notify();
        self.sync(cx);
    }

    pub(crate) fn tree_key(&mut self, key: Key, cx: &mut Context<Self>) {
        let rows: Vec<Row> = self
            .tree_rows()
            .into_iter()
            .filter(|row| matches!(row.slot, Slot::Entry { .. }))
            .collect();
        if rows.is_empty() {
            return;
        }
        let cursor = self
            .nav
            .cursor
            .clone()
            .or_else(|| self.nav.blob.as_ref().map(|(path, _)| path.clone()));
        let at = cursor
            .and_then(|path| rows.iter().position(|row| row.path == path))
            .unwrap_or(0);
        let row = &rows[at];
        let open = self.nav.expanded.contains(&row.path);
        self.nav.cursor = Some(row.path.clone());
        self.tree_walked = true;
        match key {
            Key::Up => self.nav.cursor = Some(rows[at.saturating_sub(1)].path.clone()),
            Key::Down => self.nav.cursor = Some(rows[(at + 1).min(rows.len() - 1)].path.clone()),
            Key::Home => self.nav.cursor = Some(rows[0].path.clone()),
            Key::End => self.nav.cursor = Some(rows[rows.len() - 1].path.clone()),
            Key::Right if row.is_dir() && !open => return self.toggle_dir(row.path.clone(), cx),
            Key::Right if row.is_dir() => {
                if let Some(child) = rows.get(at + 1).filter(|next| next.depth > row.depth) {
                    self.nav.cursor = Some(child.path.clone());
                }
            }
            Key::Left if row.is_dir() && open => return self.toggle_dir(row.path.clone(), cx),
            Key::Left => {
                if let Some(parent) = rows[..at].iter().rev().find(|up| up.depth < row.depth) {
                    self.nav.cursor = Some(parent.path.clone());
                }
            }
            Key::Enter if row.is_dir() => return self.toggle_dir(row.path.clone(), cx),
            Key::Enter => {
                if let Slot::Entry { oid, .. } = &row.slot {
                    return self.open_file(row.path.clone(), oid.clone(), cx);
                }
            }
            Key::Right => {}
        }
        let all = self.tree_rows();
        if let Some(at) = all
            .iter()
            .position(|row| Some(&row.path) == self.nav.cursor.as_ref())
        {
            self.tree_scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        }
        cx.notify();
    }
}

fn placeholder(dir: &[u8], depth: usize, slot: Slot) -> Row {
    let mut path = dir.to_vec();
    path.extend_from_slice(b"/...");
    Row {
        path,
        name: String::new(),
        depth,
        slot,
    }
}
