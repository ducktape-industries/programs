//! Forge on a phone: the Expo app's forge stack (`app/(tabs)/forge/**`,
//! `_layout.tsx`), a tree of its own that the view's root picks once
//! under a finger (`lib.rs`). It shares the model with the desktop
//! (`state`, `select`, `navigate`, `actions`) and draws with the SDK's
//! touch ladder; nothing in it asks which pointer it is under.
//!
//! One file per Expo screen, each naming its source: [`repos`] (Forge's
//! root), [`repo`] (a repository) with its tab bodies in [`tabs`],
//! [`change`] (a change), [`diff`] (one file of its diff), [`commit`],
//! [`code`] (one file of the Code tab), [`new_change`] (New or Edit
//! change) and [`review`] (Finish your review); [`document`] is the
//! markdown they render, [`lines`] the code and diff rows, [`parts`] what
//! several of them draw.
//!
//! Reads the route ([`Nav::pages`](crate::state::Nav::pages)), what lies
//! over the page ([`Forge::overlay`]) and the open form; writes Back.
mod change;
mod code;
mod commit;
mod diff;
mod document;
mod lines;
mod new_change;
mod parts;
mod repo;
mod repos;
mod review;
mod tabs;

use view::design::{Ladder, Nav};
use view::prelude::*;

use crate::Forge;
use crate::id;
use crate::state::{Menu, Overlay, Page, Sheet};
use crate::words;

pub(crate) use lines::Block;

/// The phone's screen: the top page of the route (or the change form
/// over it), the one overlay over that, and Back while there is somewhere
/// to go back to (rule 13): the overlay drawn, the form, or a page.
pub(crate) fn render(forge: &mut Forge, cx: &mut Context<Forge>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::TOUCH;
    let page = page(forge, cx, ladder, &theme);
    let sheet = forge
        .overlay()
        .and_then(|overlay| sheet(overlay, forge, cx, ladder, &theme));
    let somewhere = sheet.is_some() || forge.change_form().is_some() || forge.nav().depth() > 0;
    div()
        .id(id("forge"))
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .when(somewhere, |root| {
            root.on_back(cx.listener(|forge, _: &BackEvent, window, cx| forge.back(window, cx)))
        })
        .child(page)
        .children(sheet)
        .into_any_element()
}

/// The page on top: the change form while one is open, else the route's
/// top page.
fn page(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    if let Some(form) = forge.change_form() {
        return new_change::render(forge, form, cx, ladder, theme);
    }
    let nav = match forge.nav().depth() {
        0 => Nav::root(reader(forge)),
        depth => Nav::back(
            depth,
            cx.listener(|forge, _: &ClickEvent, window, cx| forge.back(window, cx)),
        ),
    };
    match forge.nav().pages().last().copied().unwrap_or(Page::Repos) {
        Page::Repos => repos::render(forge, nav, cx, ladder, theme),
        Page::Repo => repo::render(forge, nav, cx, ladder, theme),
        Page::Change => change::render(forge, nav, cx, ladder, theme),
        Page::File => diff::render(forge, nav, cx, ladder, theme),
        Page::Commit => commit::render(forge, nav, cx, ladder, theme),
        Page::Folder => code::folder(forge, nav, cx, ladder, theme),
        Page::Blob => code::render(forge, nav, cx, ladder, theme),
        Page::Missing => code::missing(forge, nav, cx, ladder, theme),
        Page::Landing => code::landing(forge, nav, cx, ladder, theme),
    }
}

/// The reader's account name, for the root's avatar: empty until the
/// roster names them, when the avatar draws `•`.
fn reader(forge: &Forge) -> String {
    match (forge.me_principal(), forge.names.ready()) {
        (Some(me), Some(names)) => names.name(&me).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

/// The overlay's sheet, as long as what it shows is held: a ref picker, a change's actions, a member to
/// grant or to ask for a review, About, New repository, or Finish your
/// review.
fn sheet(
    overlay: Overlay,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    Some(match overlay {
        Overlay::Menu(menu) => {
            let title = match menu {
                Menu::Ref => words::PICK_A_REF,
                Menu::Head => words::DEFAULT_HEAD,
                Menu::Source => "Source",
                Menu::Target => "Target",
            };
            parts::ref_sheet(menu, title, forge, cx, ladder, theme)
        }
        Overlay::Sheet(Sheet::About) => repo::about_sheet(forge, cx, ladder, theme),
        Overlay::Sheet(Sheet::ChangeActions) => change::actions_sheet(forge, cx, ladder, theme),
        Overlay::Sheet(Sheet::Grant) => tabs::grant_sheet(forge, cx, ladder, theme),
        Overlay::Sheet(Sheet::Reviewers) => new_change::reviewers_sheet(forge, cx, ladder, theme),
        Overlay::NewRepository => {
            repos::new_repository(forge.new_repo.as_ref()?, cx, ladder, theme)
        }
        Overlay::FinishReview => review::render(forge, forge.review()?, cx, ladder, theme),
        Overlay::Policy => crate::policies::sheet(forge, cx, ladder, theme)?,
    })
}
