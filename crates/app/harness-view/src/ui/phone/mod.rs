//! Agents under a finger: the phone's pages, in the kit's touch look (`design::TOUCH`). The
//! root picks this tree once, from the device (`lib.rs`); nothing in it asks again. The screens
//! are the desktop's, drawn as a stack of pages whose depth the screen fixes: the agents are the
//! root; an agent, the models and the create form go over it; a conversation, a part of an
//! agent's setup and the model form go over those; a conversation's usage over that. A page's
//! writes open as a sheet from its title row, and a frame's facts as a sheet from a long press.
//! Reads what the desktop reads; writes the screen, the sheet and the folds.
//!
//! - [`agents`]: the root, the reader's agents and the network's.
//! - [`agent`]: one agent, its setup's pages and its manager's sheet.
//! - [`thread`]: one conversation, its usage page and its sheets.
//! - [`models`]: the model records, and the form that sets one.
//! - [`create`]: the create and edit form.
//! - [`parts`]: what two or more of them draw the same way.
use view::design::{Ladder, Nav, TOUCH};
use view::prelude::*;

use crate::{Agents, Screen};

mod agent;
mod agents;
mod create;
mod models;
mod parts;
mod thread;

/// The page the screen is, and the sheet over it; Back undoes what is drawn: the sheet, else
/// the page, and at the root the host leaves the program.
pub(crate) fn render(view: &Agents, cx: &mut Context<Agents>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &TOUCH;
    let (page, sheet) = match view.screen {
        Screen::List => (agents::page(view, cx, &theme, ladder), None),
        Screen::Agent(agent) => (
            agent::page(view, agent, cx, &theme, ladder),
            agent::sheet(view, agent, cx, &theme, ladder),
        ),
        Screen::Part(agent, part) => (agent::part(view, agent, part, cx, &theme, ladder), None),
        Screen::Thread(id) => (
            thread::page(view, id, cx, &theme, ladder),
            thread::sheet(view, cx, &theme, ladder),
        ),
        Screen::Usage(id) => (thread::usage(view, id, cx, &theme, ladder), None),
        Screen::Models => (models::page(view, cx, &theme, ladder), None),
        Screen::SetModel => (models::form(view, cx, &theme, ladder), None),
        Screen::Create => (create::page(view, cx, &theme, ladder), None),
    };
    let sheet = crate::policies::sheet(view, cx, ladder, &theme).or(sheet);
    let back = (sheet.is_some() || depth(view) > 0)
        .then(|| cx.listener(|view: &mut Agents, _: &BackEvent, _, cx| view.back(cx)));
    div()
        .id("agents")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .child(page)
        .children(sheet)
        .when_some(back, |root, back| root.on_back(back))
        .into_any_element()
}

/// How many pages the screen sits over: the agents are the root.
fn depth(view: &Agents) -> usize {
    match view.screen {
        Screen::List => 0,
        Screen::Agent(_) | Screen::Models => 1,
        Screen::Create => match view.draft.again {
            true => 2,
            false => 1,
        },
        Screen::Thread(_) | Screen::Part(..) | Screen::SetModel => 2,
        Screen::Usage(_) => 3,
    }
}

/// Where the page sits: the root leads with the reader (a visitor has none to show), every
/// other page has Back.
fn nav(view: &Agents, cx: &mut Context<Agents>) -> Nav {
    match (depth(view), view.me) {
        (0, Some(_)) => Nav::root(view.home().unwrap_or_default().to_owned()),
        (0, None) => Nav::Child,
        (depth, _) => Nav::back(
            depth,
            cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.back(cx)),
        ),
    }
}

/// A page of the stack: its title row (Back by where it sits), what the last write left under
/// it, then what the caller adds.
fn page(
    id: &'static str,
    title: impl Into<SharedString>,
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> design::Page {
    let nav = nav(view, cx);
    design::page(id, title, nav, ladder, theme).children(parts::note(view, theme, ladder))
}
