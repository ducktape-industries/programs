//! Where the reader is: every event that moves between screens, tabs,
//! files and panels. A move clears the notice and re-syncs the reads.
use view::prelude::*;

use crate::Stage;
use crate::markdown::Target;
use crate::state::{
    ChangeTab, Dock, Filter, Forge, Menu, Overlay, Page, RepoTab, SettingsForm, Sheet, change_key,
};
use crate::words;
use forge::Reply;

impl Forge {
    fn moved(&mut self, cx: &mut Context<Self>) {
        self.notice.clear();
        self.nav.gone = None;
        self.sheet = None;
        self.details = false;
        self.repo_policy.sheet = None;
        self.change_policy.sheet = None;
        self.settings_policy.sheet = None;
        self.list_cursor = None;
        self.repos_cursor = None;
        self.rail_cursor = None;
        self.diff_cursor = None;
        self.tree_walked = false;
        self.reveal = None;
        cx.notify();
        self.sync(cx);
    }

    pub(crate) fn open_repos(&mut self, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.moved(cx);
    }

    pub(crate) fn overlay(&self) -> Option<Overlay> {
        if self.policy_sheet_open() {
            return Some(Overlay::Policy);
        }
        if let Some(menu) = self.menu {
            return Some(Overlay::Menu(menu));
        }
        if let Some(sheet) = self.sheet {
            return Some(Overlay::Sheet(sheet));
        }
        if self.new_repo.is_some() {
            return Some(Overlay::NewRepository);
        }
        let finishing = self.review().is_some_and(|review| review.finishing);
        finishing.then_some(Overlay::FinishReview)
    }

    pub(crate) fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.overlay() {
            Some(Overlay::Menu(menu)) => return self.close_dropdown(menu, window, cx),
            Some(Overlay::Sheet(_)) => return self.close_sheet(cx),
            Some(Overlay::NewRepository) => return self.cancel_repo(cx),
            Some(Overlay::FinishReview) => return self.finishing(false, cx),
            Some(Overlay::Policy) => return self.close_policy_sheet(cx),
            None => {}
        }
        if self.change_form().is_some() {
            return self.cancel_change(cx);
        }
        match self.nav.pages().last().copied().unwrap_or(Page::Repos) {
            Page::Repos => {}
            Page::Repo => self.open_repos(cx),
            Page::Change => self.open_change(None, cx),
            Page::File => self.single_file(None, cx),
            Page::Commit => self.open_commit(None, cx),
            Page::Folder => {
                let up = self.nav.folder.as_deref().and_then(|path| {
                    let at = path.iter().rposition(|byte| *byte == b'/')?;
                    Some(path[..at].to_vec())
                });
                self.open_folder(up, cx);
            }
            Page::Blob | Page::Missing => self.nav_close_blob(cx),
            Page::Landing => {
                self.nav.goto = None;
                self.nav.linked = None;
                self.moved(cx);
            }
        }
    }

    pub(crate) fn open_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        self.sheet = Some(sheet);
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.closing = None;
        cx.notify();
    }

    pub(crate) fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        match route.split('/').collect::<Vec<_>>().as_slice() {
            [name] if !name.is_empty() => self.open_repo((*name).to_owned(), cx),
            [name, n] if !name.is_empty() && ducklink::number(n).is_some() => {
                self.open_repo((*name).to_owned(), cx);
                self.open_change(ducklink::number(n), cx);
            }
            [name, "blob" | "tree", first, rest @ ..] if !name.is_empty() => {
                self.open_repo((*name).to_owned(), cx);
                self.nav.tab = RepoTab::Code;
                let segments = std::iter::once(first).chain(rest);
                self.nav.linked = Some(segments.map(|segment| (*segment).to_owned()).collect());
                cx.notify();
                self.sync(cx);
            }
            [name, path @ ..] if !name.is_empty() && !path.is_empty() => {
                self.open_repo((*name).to_owned(), cx);
                self.open_path(path.join("/").into_bytes(), cx);
            }
            _ => self.open_repos(cx),
        }
    }

    pub(crate) fn land_link(&mut self, cx: &mut Context<Self>) {
        let Some(segments) = self.nav.linked.clone() else {
            return;
        };
        let refs = match self.stage(&self.refs_query()) {
            Stage::Loading => return,
            Stage::Failed(refusal) => Err(design::sentence(refusal)),
            Stage::Ready(Reply::Refs { page, .. }) => Ok(page.items.clone()),
            Stage::Ready(_) => return,
        };
        let hash = match self.stage(&self.repo_query()) {
            Stage::Loading => return,
            Stage::Failed(refusal) => Err(design::sentence(refusal)),
            Stage::Ready(Reply::Repo { repo, .. }) => Ok(repo.repo.hash),
            Stage::Ready(_) => return,
        };
        let (refs, hash) = match (refs, hash) {
            (Ok(refs), Ok(hash)) => (refs, hash),
            (Err(said), _) | (_, Err(said)) => {
                self.nav.linked = None;
                self.notice = said;
                cx.notify();
                return;
            }
        };
        if let Some((rev, path)) = linked(&segments, &refs, oid_hex(hash)) {
            self.nav.linked = None;
            self.nav.rev = Some(rev);
            return self.land_path(path, cx);
        }
        let names: Vec<Vec<u8>> = match self.tree_query(Vec::new()) {
            None => Vec::new(),
            Some(root) => match self.stage(&root) {
                Stage::Loading => return,
                Stage::Ready(Reply::Tree { page, .. }) => {
                    page.items.iter().map(|entry| entry.name.clone()).collect()
                }
                Stage::Failed(_) | Stage::Ready(_) => Vec::new(),
            },
        };
        let at = (1..segments.len())
            .find(|at| names.iter().any(|name| name == segments[*at].as_bytes()))
            .unwrap_or(1)
            .min(segments.len());
        self.nav.linked = None;
        self.open_tab(RepoTab::Code, cx);
        self.nav.gone = Some((segments[..at].join("/"), segments[at..].join("/")));
        cx.notify();
    }

    pub(crate) fn open_gone(&mut self, cx: &mut Context<Self>) {
        let Some((_, path)) = self.nav.gone.take() else {
            return;
        };
        self.nav.rev = None;
        self.land_path(path, cx);
    }

    pub(crate) fn blob_route(&self, path: &str) -> Vec<String> {
        let path: Vec<String> = path.split('/').map(str::to_owned).collect();
        let rev = self.head_name();
        if rev == self.default_head() {
            return path;
        }
        let named: Vec<String> = words::ref_label(&rev)
            .split('/')
            .map(str::to_owned)
            .collect();
        let whole: Vec<String> = named.iter().chain(&path).cloned().collect();
        let refs = self.refs().unwrap_or_default();
        let hex = self.repo().map_or(40, |(info, _)| oid_hex(info.repo.hash));
        let back = linked(&whole, refs, hex);
        let round = back == Some((rev, path.join("/")));
        let first = match (round, self.head_oid()) {
            (false, Some(oid)) => vec![oid],
            _ => named,
        };
        std::iter::once("blob".to_owned())
            .chain(first)
            .chain(path)
            .collect()
    }

    fn land_path(&mut self, path: String, cx: &mut Context<Self>) {
        match path.is_empty() {
            true => self.open_tab(RepoTab::Code, cx),
            false => self.open_path(path.into_bytes(), cx),
        }
    }

    pub(crate) fn open_repo(&mut self, name: String, cx: &mut Context<Self>) {
        self.nav = Default::default();
        self.nav.repo = Some(name);
        self.moved(cx);
    }

    pub(crate) fn open_tab(&mut self, tab: RepoTab, cx: &mut Context<Self>) {
        self.menu = None;
        self.tab_cursor = None;
        let kept = std::mem::take(&mut self.nav);
        self.nav.repo = kept.repo;
        self.nav.rev = kept.rev;
        self.nav.expanded = kept.expanded;
        self.nav.cursor = kept.cursor;
        self.nav.blob = kept.blob;
        self.nav.folder = kept.folder;
        self.nav.goto = kept.goto;
        self.nav.tab = tab;
        if self.touch && tab != RepoTab::Code {
            self.nav.folder = None;
            self.nav.blob = None;
        }
        if tab == RepoTab::Settings && !self.touch {
            let head = self.default_head();
            let (allow_force, allow_delete) = self
                .repo()
                .map(|(info, _)| {
                    (
                        info.repo.settings.allow_force,
                        info.repo.settings.allow_delete,
                    )
                })
                .unwrap_or((false, false));
            self.repo_settings = Some(SettingsForm {
                head,
                allow_force,
                allow_delete,
                grant: Default::default(),
            });
        }
        self.moved(cx);
    }

    pub(crate) fn open_menu(&mut self, menu: Option<Menu>, cx: &mut Context<Self>) {
        self.menu = menu;
        cx.notify();
    }

    pub(crate) fn open_dropdown(
        &mut self,
        menu: Menu,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = event.position();
        self.menu_at = (at.x.into(), at.y.into());
        self.menu_search.reset("");
        let picked = self.menu_picked(menu);
        self.menu_tags = self.menu_offers_tags(menu) && picked.starts_with(b"refs/tags/");
        let index = self.menu_list(menu).iter().position(|name| *name == picked);
        self.menu_cursor = index.unwrap_or(0);
        self.menu_scroll
            .scroll_to_item(self.menu_cursor, ScrollStrategy::Center);
        self.open_menu(Some(menu), cx);
        if !self.touch {
            window.focus(crate::id(format!("{}-filter", menu.key())));
        }
    }

    pub(crate) fn menu_step(&mut self, index: usize, cx: &mut Context<Self>) {
        self.menu_cursor = index;
        self.menu_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        cx.notify();
    }

    pub(crate) fn menu_pick(
        &mut self,
        menu: Menu,
        name: Vec<u8>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match menu {
            Menu::Ref => self.pick_ref(name, cx),
            Menu::Head if self.touch => self.configure_with(|settings| settings.head = name, cx),
            Menu::Head => {
                if let Some(form) = &mut self.repo_settings {
                    form.head = name;
                }
            }
            Menu::Source | Menu::Target => {
                if let Some(form) = &mut self.form {
                    match menu {
                        Menu::Source => form.from = name,
                        _ => form.into = name,
                    }
                }
                self.sync(cx);
            }
        }
        self.close_dropdown(menu, window, cx);
    }

    pub(crate) fn close_dropdown(
        &mut self,
        menu: Menu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu = None;
        window.focus(crate::id(menu.key().to_owned()));
        cx.notify();
    }

    pub(crate) fn pick_ref(&mut self, name: Vec<u8>, cx: &mut Context<Self>) {
        self.menu = None;
        self.nav.rev = Some(name);
        self.nav.expanded.clear();
        self.nav.cursor = None;
        self.nav.blob = None;
        self.nav.folder = None;
        self.nav.missing = None;
        self.nav.commit = None;
        self.moved(cx);
    }

    pub(crate) fn open_folder(&mut self, path: Option<Vec<u8>>, cx: &mut Context<Self>) {
        self.tree_search.reset("");
        self.nav.folder = path;
        self.nav.blob = None;
        self.nav.missing = None;
        self.moved(cx);
    }

    pub(crate) fn open_file(&mut self, path: Vec<u8>, oid: String, cx: &mut Context<Self>) {
        self.wrap = false;
        self.source = false;
        self.nav.cursor = Some(path.clone());
        self.nav.blob = Some((path, oid));
        self.moved(cx);
    }

    pub(crate) fn follow_link(&mut self, dir: &[u8], dest: &str, cx: &mut Context<Self>) {
        match crate::markdown::target(dir, dest) {
            Some(Target::Web(url)) => cx.host().open_link(&url),
            Some(Target::Path(path)) => self.open_path(path, cx),
            None => {}
        }
    }

    pub(crate) fn open_path(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        if self.readme().is_some_and(|(name, _)| name == path) {
            return self.open_tab(RepoTab::Readme, cx);
        }
        if self.touch {
            self.nav.folder = path
                .iter()
                .rposition(|b| *b == b'/')
                .map(|at| path[..at].to_vec());
            self.nav.blob = None;
        } else {
            let mut dir = &path[..];
            while let Some(at) = dir.iter().rposition(|b| *b == b'/') {
                dir = &dir[..at];
                self.nav.expanded.insert(dir.to_vec());
            }
        }
        self.nav.goto = Some(path);
        self.open_tab(RepoTab::Code, cx);
    }

    pub(crate) fn land_goto(&mut self) {
        let Some(path) = self.nav.goto.clone() else {
            return;
        };
        let split = path.iter().rposition(|b| *b == b'/');
        let (dir, name) = match split {
            Some(at) => (path[..at].to_vec(), &path[at + 1..]),
            None => (Vec::new(), &path[..]),
        };
        let Some(query) = self.tree_query(dir) else {
            return;
        };
        let entry = match self.stage(&query) {
            Stage::Loading => return,
            Stage::Failed(_) => None,
            Stage::Ready(Reply::Tree { page, .. }) => page
                .items
                .iter()
                .find(|entry| entry.name == name)
                .map(|entry| (entry.kind, entry.oid.clone())),
            Stage::Ready(_) => None,
        };
        self.nav.goto = None;
        self.nav.cursor = Some(path.clone());
        match entry {
            Some((forge::EntryKind::Directory, _)) if self.touch => self.nav.folder = Some(path),
            Some((forge::EntryKind::Directory, _)) => {
                self.nav.expanded.insert(path);
            }
            Some((_, oid)) => self.nav.blob = Some((path, oid)),
            None if self.touch => self.nav.missing = Some(path),
            None => self.notice = words::no_path(&path),
        }
    }

    pub(crate) fn nav_close_blob(&mut self, cx: &mut Context<Self>) {
        self.nav.blob = None;
        self.nav.missing = None;
        self.nav.gone = None;
        self.moved(cx);
    }

    pub(crate) fn open_commit(&mut self, oid: Option<String>, cx: &mut Context<Self>) {
        self.nav.diff_path = None;
        self.nav.commit = oid;
        self.moved(cx);
    }

    pub(crate) fn open_change(&mut self, n: Option<u64>, cx: &mut Context<Self>) {
        self.menu = None;
        self.nav.change = n;
        self.nav.change_tab = ChangeTab::default();
        self.nav.diff_path = None;
        self.nav.all_files = false;
        self.nav.dock = None;
        self.layout.dock_open = false;
        self.closing = None;
        self.reply.reset("");
        self.moved(cx);
    }

    pub(crate) fn open_change_tab(&mut self, tab: ChangeTab, cx: &mut Context<Self>) {
        self.change_tab_cursor = None;
        self.nav.change_tab = tab;
        self.nav.diff_path = None;
        self.nav.all_files = false;
        self.nav.commit = None;
        self.moved(cx);
    }

    pub(crate) fn open_details(&mut self, cx: &mut Context<Self>) {
        self.change_tab_cursor = None;
        self.details = true;
        cx.notify();
    }

    pub(crate) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
        self.filter = filter;
        self.moved(cx);
    }

    pub(crate) fn toggle_dock(&mut self, dock: Dock, cx: &mut Context<Self>) {
        self.nav.dock = (self.nav.dock != Some(dock)).then_some(dock);
        self.layout.dock_open = self.nav.dock.is_some();
        cx.notify();
    }

    pub(crate) fn single_file(&mut self, path: Option<Vec<u8>>, cx: &mut Context<Self>) {
        let lists = [crate::ui::change::FILE_LIST_ID, crate::ui::diff::STRIP_ID];
        if path.is_none() && matches!(self.list_cursor, Some((on, ..)) if lists.contains(&on)) {
            self.list_cursor = None;
        }
        if path.is_none() {
            self.nav.all_files = false;
        }
        self.reveal = path.clone();
        self.land_reveal();
        self.nav.diff_path = path;
        cx.notify();
    }

    pub(crate) fn open_all_files(&mut self, cx: &mut Context<Self>) {
        self.nav.diff_path = None;
        self.nav.all_files = true;
        cx.notify();
    }

    pub(crate) fn open_file_comment(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        self.nav.change_tab = ChangeTab::Files;
        self.nav.diff_path = Some(path.clone());
        self.moved(cx);
        self.reveal = Some(path);
        self.land_reveal();
    }

    pub(crate) fn land_reveal(&mut self) {
        let Some(path) = self.reveal.clone() else {
            return;
        };
        if self.nav.change.is_none() || self.nav.change_tab != ChangeTab::Files {
            self.reveal = None;
            return;
        }
        let Some(query) = self.diff_query() else {
            return;
        };
        let at = match self.stage(&query) {
            Stage::Loading => return,
            Stage::Ready(Reply::Diff { page, .. }) => page
                .items
                .iter()
                .position(|file| crate::select::path_of(file).as_ref() == Some(&path)),
            Stage::Ready(_) | Stage::Failed(_) => None,
        };
        self.reveal = None;
        if let Some(at) = at {
            self.files_scroll
                .scroll_to_item(at, ScrollStrategy::Nearest);
            self.list_cursor = Some((crate::ui::change::FILE_LIST_ID, at, 0));
        }
    }

    pub(crate) fn toggle_viewed(&mut self, path: &[u8], cx: &mut Context<Self>) {
        let Some(key) = self.file_key(path) else {
            return;
        };
        if !self.viewed.remove(&key) {
            self.viewed.insert(key);
        }
        cx.notify();
    }

    pub(crate) fn file_key(&self, path: &[u8]) -> Option<String> {
        Some(format!(
            "{}:{}",
            change_key(self.nav().repo.as_deref()?, self.nav().change?),
            String::from_utf8_lossy(path)
        ))
    }
}

fn linked(segments: &[String], refs: &[forge::RefInfo], hex: usize) -> Option<(Vec<u8>, String)> {
    let named = (1..=segments.len()).rev().find_map(|at| {
        let name = segments[..at].join("/");
        ["refs/heads/", "refs/tags/"]
            .iter()
            .map(|kind| format!("{kind}{name}").into_bytes())
            .find(|full| refs.iter().any(|info| info.name == *full))
            .map(|full| (full, segments[at..].join("/")))
    });
    named.or_else(|| {
        let first = segments.first()?;
        let commit = first.len() == hex
            && first
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        commit.then(|| (first.clone().into_bytes(), segments[1..].join("/")))
    })
}

fn oid_hex(hash: abi::HashKind) -> usize {
    match hash {
        abi::HashKind::Sha1 => 40,
        abi::HashKind::Sha256 => 64,
    }
}
