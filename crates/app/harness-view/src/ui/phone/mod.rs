//! Agents under a finger, in the kit's touch look: the desktop's screens as a stack of pages whose
//! depth the screen fixes (`agents`, `agent`, `thread`, `routes`, `create`, with `parts` they share);
//! a page's writes open as a sheet from its title row, a frame's facts from a long press.
use view::design::{Ladder, Nav, TOUCH};
use view::prelude::*;

use crate::{Agents, Screen};

mod agent;
mod agents;
mod create;
mod parts;
mod routes;
mod thread;

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
        Screen::Routes => (routes::page(view, cx, &theme, ladder), None),
        Screen::SetRoute => (routes::form(view, cx, &theme, ladder), None),
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

fn depth(view: &Agents) -> usize {
    match view.screen {
        Screen::List => 0,
        Screen::Agent(_) | Screen::Routes => 1,
        Screen::Create => match view.draft.again {
            true => 2,
            false => 1,
        },
        Screen::Thread(_) | Screen::Part(..) | Screen::SetRoute => 2,
        Screen::Usage(_) => 3,
    }
}

fn nav(view: &Agents, cx: &mut Context<Agents>) -> Nav {
    match (depth(view), view.me) {
        (0, Some(me)) => Nav::root_with(
            view.home().unwrap_or_default().to_owned(),
            view.picture(me, cx),
        ),
        (0, None) => Nav::Child,
        (depth, _) => Nav::back(
            depth,
            cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.back(cx)),
        ),
    }
}

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
