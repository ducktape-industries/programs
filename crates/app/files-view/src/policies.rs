//! Who writes the path on screen: its nearest guard (files' `Guarded`), whose policy ownership
//! keeps (through `policy_view::item`), Guard for the path's owner where it keeps none of its own,
//! and the writers a new path's form picked, set on its own guard once the path is made.
use ownership::{Ownership, Writers};
use policy_view::{Editing, Item, item, link};
use view::design::Ladder;
use view::methods::{Change, Submitted};
use view::prelude::*;
use view::{Div, Stateful};

use crate::state::{Guard, Note, Route};
use crate::{Files, words};

const ROW: &str = "files-policy";
const GUARD: &str = "files/guard";

pub(crate) fn path_policy(view: &mut Files) -> &mut Editing {
    &mut view.path_policy
}

pub(crate) fn guard_item(path: &str) -> Item {
    Item {
        program: files::PROGRAM.to_owned(),
        key: files::guard_key(path),
        label: path.to_owned(),
    }
}

impl Files {
    fn shown_guard(&self) -> Option<(&str, &Guard)> {
        match self.page() {
            Route::Folder(path) => {
                let guard = self.folder_at(path)?.guard.as_ref()?;
                Some((path.as_str(), guard))
            }
            Route::File(path) => {
                let guard = self.file_at(path)?.guard.as_ref()?;
                Some((path.as_str(), guard))
            }
            Route::Places | Route::History | Route::Snapshot(_) => None,
        }
    }

    pub(crate) fn settle_policy(&mut self, cx: &mut Context<Self>) {
        let held = self.shown_guard().and_then(|(_, guard)| guard.at.clone());
        let reader = self.reader.account.map(ownership::Party::Account);
        match held {
            Some(at) => item::hold(self, path_policy, guard_item(&at), reader, cx),
            None => item::let_go(self, path_policy),
        }
    }

    pub(crate) fn guardable(&self) -> Option<String> {
        let (path, guard) = self.shown_guard()?;
        let own = guard.at.as_deref() == Some(path);
        (self.reader.owns(guard) && !own).then(|| path.to_owned())
    }

    pub(crate) fn guard(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.guardable() else {
            return;
        };
        let ask = cx
            .host()
            .ask::<Submit<files::Files>>(files::Op::Guard { path });
        cx.land(ask, |view, answer, cx| view.written(answer, cx))
            .detach();
    }

    pub(crate) fn narrow(&mut self, path: String, write: Writers, cx: &mut Context<Self>) {
        let Some(account) = self.reader.account else {
            return;
        };
        let host = cx.host();
        let work = async move {
            if let Some(listed) = link::access_list(&host, files::PROGRAM, &path, &write).await?
                && !link::landed(&listed)
            {
                return Ok(listed);
            }
            let guard = files::Op::Guard { path: path.clone() };
            let guarded = host.ask::<Submit<files::Files>>(guard).await?;
            if !link::landed(&guarded) {
                return Ok(guarded);
            }
            let owner = ownership::Party::Account(account);
            link::link(&host, guard_item(&path), owner, write).await
        };
        cx.land(work, |view, answer, cx| view.written(answer, cx))
            .detach();
    }

    fn written(&mut self, answer: Result<Submitted, Error>, cx: &mut Context<Self>) {
        match design::failure(&answer) {
            None => self.read_all(cx),
            Some(problem) => self.note = Some(Note::Refused(problem)),
        }
        cx.notify();
    }

    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        item::moved(self, path_policy, change.as_ref(), cx);
        let folder = self.folder.ready().and_then(|folder| folder.guard.as_ref());
        let file = self.file.ready().and_then(|file| file.guard.as_ref());
        let touched = [folder, file]
            .into_iter()
            .flatten()
            .filter_map(|guard| guard.at.as_deref())
            .any(|at| {
                let key = files::guard_key(at);
                let program = files::PROGRAM.to_owned();
                let record = ownership::Query::Record {
                    program: program.clone(),
                    key: key.clone(),
                };
                let effective = ownership::Query::Effective { program, key };
                change.as_ref().is_none_or(|change| {
                    change.touches::<Ownership, _>(&record)
                        || change.touches::<Ownership, _>(&effective)
                })
            });
        if touched {
            self.read_all(cx);
        }
    }

    pub(crate) fn policy_sheet_open(&self) -> bool {
        self.path_policy.sheet.is_some()
    }

    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        item::close_sheet(self, path_policy, cx);
    }
}

pub(crate) fn row(
    view: &Files,
    cx: &mut Context<Files>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let (_, guard) = view.shown_guard()?;
    let row = match &guard.at {
        Some(_) => item::row(&view.path_policy, ROW, path_policy, cx, ladder, theme)?,
        None => open_row(ladder, theme),
    };
    let guarding = view.guardable().map(|_| {
        let press = cx.listener(|view: &mut Files, _: &ClickEvent, _, cx| view.guard(cx));
        div().px_5().py_2().child(
            design::button(GUARD.to_owned(), words::GUARD, ladder, theme, press)
                .kind(design::Kind::Outline),
        )
    });
    Some(
        div()
            .flex()
            .flex_col()
            .child(row)
            .children(guarding)
            .into_any_element(),
    )
}

fn open_row(ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let said = div()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(words::ANYONE_WRITES);
    design::row(ROW.to_owned(), ladder, theme)
        .min_h(ladder.row_tall)
        .flex_none()
        .child(design::lead(
            design::icon("shield", ladder.icon_row).text_color(theme.muted),
        ))
        .child(design::row_words(design::row_title(item::WHO, ladder, theme)).child(said))
}

pub(crate) fn sheet(
    view: &Files,
    cx: &mut Context<Files>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    item::sheet(&view.path_policy, ROW, path_policy, cx, ladder, theme)
}
