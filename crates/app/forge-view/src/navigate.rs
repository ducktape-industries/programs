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
        self.sheet = None;
        self.details = false;
        // a walk of a list ends where the reader moves: none stays lit
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

    /// What lies over the phone's page, the one sheet at a time: a ref
    /// picker, a sheet a page opened (About among them), New repository,
    /// or Finish your review. The phone draws this and its Back closes
    /// this, so the two never disagree (rule 13).
    pub(crate) fn overlay(&self) -> Option<Overlay> {
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

    /// The phone's Back: the overlay closes, else the open change form;
    /// else the top page pops to the one under it
    /// ([`crate::state::Nav::pages`]). Nothing at the root.
    pub(crate) fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.overlay() {
            Some(Overlay::Menu(menu)) => return self.close_dropdown(menu, window, cx),
            Some(Overlay::Sheet(_)) => return self.close_sheet(cx),
            Some(Overlay::NewRepository) => return self.cancel_repo(cx),
            Some(Overlay::FinishReview) => return self.finishing(false, cx),
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
                // one folder up, or the tab's root
                let up = self.nav.folder.as_deref().and_then(|path| {
                    let at = path.iter().rposition(|byte| *byte == b'/')?;
                    Some(path[..at].to_vec())
                });
                self.open_folder(up, cx);
            }
            Page::Blob | Page::Missing => self.nav_close_blob(cx),
            // the link is forgotten, so its read landing later moves nothing
            Page::Landing => {
                self.nav.goto = None;
                self.moved(cx);
            }
        }
    }

    pub(crate) fn open_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        self.sheet = Some(sheet);
        cx.notify();
    }

    /// The sheet closes, and a Close it was asking about again with it:
    /// the next Actions opens on its list.
    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet = None;
        self.closing = None;
        cx.notify();
    }

    /// A `host.route` item: `<name>` opens that repository, `<name>/<n>`
    /// its change `n` (the path chat links a change's room by, its id
    /// `forge:<name>:<n>`), `<name>/<path…>` that file or folder of its
    /// Code tab (a file page's Copy link) at the path's depth; anything
    /// else opens the list.
    pub(crate) fn open_route(&mut self, route: &str, cx: &mut Context<Self>) {
        match route.split('/').collect::<Vec<_>>().as_slice() {
            [name] if !name.is_empty() => self.open_repo((*name).to_owned(), cx),
            [name, n] if !name.is_empty() && ducklink::number(n).is_some() => {
                self.open_repo((*name).to_owned(), cx);
                self.open_change(ducklink::number(n), cx);
            }
            [name, path @ ..] if !name.is_empty() && !path.is_empty() => {
                self.open_repo((*name).to_owned(), cx);
                self.open_path(path.join("/").into_bytes(), cx);
            }
            _ => self.open_repos(cx),
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
        // the tree keeps what it had open across tabs
        let kept = std::mem::take(&mut self.nav);
        self.nav.repo = kept.repo;
        self.nav.rev = kept.rev;
        self.nav.expanded = kept.expanded;
        self.nav.cursor = kept.cursor;
        self.nav.blob = kept.blob;
        self.nav.folder = kept.folder;
        self.nav.goto = kept.goto;
        self.nav.tab = tab;
        // the phone's folder and file pages stand over the Code tab alone:
        // another tab (a README link from a file among them) leaves them,
        // so the Code tab opens on its root again, not on a page left under
        if self.touch && tab != RepoTab::Code {
            self.nav.folder = None;
            self.nav.blob = None;
        }
        // the desktop stages its settings in a form saved by Save; the
        // phone saves each flip from what is on screen (`configure_with`)
        if tab == RepoTab::Settings && !self.touch {
            let head = self.default_head();
            let (allow_force, allow_delete) = self
                .repo()
                .map(|(info, _, _)| {
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

    /// Opens a dropdown under its button; a press anywhere else closes it.
    pub(crate) fn open_menu(&mut self, menu: Option<Menu>, cx: &mut Context<Self>) {
        self.menu = menu;
        cx.notify();
    }

    /// Opens `menu` under the press that asked for it, its filter box
    /// empty and (under a pointer) holding the keys, on the tab and the item
    /// it has picked,
    /// scrolled to the middle; Esc or a pick gives the keys back to the button
    /// ([`crate::ui::components::dropdown`]).
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
        // centred, so the pick shows with its neighbours; arrows keep Nearest
        self.menu_scroll
            .scroll_to_item(self.menu_cursor, ScrollStrategy::Center);
        self.open_menu(Some(menu), cx);
        // under a finger the sheet opens with the keyboard down, as Expo's
        // `RefSheet` does (its filter sets no `autoFocus`)
        if !self.touch {
            window.focus(crate::id(format!("{}-filter", menu.key())));
        }
    }

    /// The arrows are on item `index` of the open dropdown, scrolled to.
    pub(crate) fn menu_step(&mut self, index: usize, cx: &mut Context<Self>) {
        self.menu_cursor = index;
        self.menu_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// `name` picked in `menu`: the ref every tab reads, the default head
    /// (the desktop's settings form; on the phone saved at once), or the
    /// change form's source or target. The menu closes and gives the keys
    /// back.
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
                // the form reads how its new ends compare
                self.sync(cx);
            }
        }
        self.close_dropdown(menu, window, cx);
    }

    /// Closes `menu` and gives the keys back to its button.
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

    /// The phone's Code tab opens the folder at `path` on its own page;
    /// `None` goes back to the tab's root.
    pub(crate) fn open_folder(&mut self, path: Option<Vec<u8>>, cx: &mut Context<Self>) {
        self.tree_search.reset("");
        self.nav.folder = path;
        self.nav.blob = None;
        self.nav.missing = None;
        self.moved(cx);
    }

    pub(crate) fn open_file(&mut self, path: Vec<u8>, oid: String, cx: &mut Context<Self>) {
        // a file opens unwrapped, a markdown one as its document
        self.wrap = false;
        self.source = false;
        self.nav.cursor = Some(path.clone());
        self.nav.blob = Some((path, oid));
        self.moved(cx);
    }

    /// A pressed link in a document whose folder is `dir`: the web through
    /// the host, a file of this repository in the Code tab (the root README
    /// in its own tab).
    pub(crate) fn follow_link(&mut self, dir: &[u8], dest: &str, cx: &mut Context<Self>) {
        match crate::markdown::target(dir, dest) {
            Some(Target::Web(url)) => cx.host().open_link(&url),
            Some(Target::Path(path)) => self.open_path(path, cx),
            None => {}
        }
    }

    /// Opens a file or folder by its path alone, once the tree that holds
    /// it says which it is: on the desktop its folders unfold in the tree;
    /// on the phone it opens on its own page over its folder's (Expo
    /// `code/[...path].tsx`).
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

    /// Lands a pending [`Self::open_path`] once its folder's tree is read:
    /// a folder opens (the phone's page, the desktop's tree row), a file
    /// opens, and a path the ref does not hold says so (the phone's page,
    /// the desktop's notice).
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
        self.moved(cx);
    }

    pub(crate) fn open_commit(&mut self, oid: Option<String>, cx: &mut Context<Self>) {
        // a commit opens on all its files, not on one another showed alone
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

    /// The phone's Details tab: the change's sidebar as a page of its own
    /// (board 23). Any move leaves it.
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
        // all files again: the file list's keys (a change's, a commit's
        // strip) start over, no row left lit
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

    /// The phone's All files: every file of the change's diff on a page
    /// of its own (Expo `diff.tsx` without a path).
    pub(crate) fn open_all_files(&mut self, cx: &mut Context<Self>) {
        self.nav.diff_path = None;
        self.nav.all_files = true;
        cx.notify();
    }

    /// A line comment pressed in the details: its file, alone, in Files.
    pub(crate) fn open_file_comment(&mut self, path: Vec<u8>, cx: &mut Context<Self>) {
        self.nav.change_tab = ChangeTab::Files;
        self.nav.diff_path = Some(path.clone());
        self.moved(cx);
        self.reveal = Some(path);
        self.land_reveal();
    }

    /// The file picked to show alone (its row, a comment in the details),
    /// scrolled into the Files tab's list and made the row its keys go on
    /// from, once the diff that lists it has landed.
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
