//! The window: a repositories rail, one content column, one docked panel.
//! Narrow windows fold the rail and the dock into toggles rather than
//! squeezing three columns into one.
pub(crate) mod change;
pub(crate) mod changes;
pub(crate) mod code;
pub(crate) mod commits;
pub(crate) mod components;
pub(crate) mod conversation;
pub(crate) mod diff;
pub(crate) mod dock;
pub(crate) mod phone;
pub(crate) mod readme;
pub(crate) mod refs;
pub(crate) mod repos;
pub(crate) mod settings;

use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::Forge;
use crate::select::Writer;
use crate::state::{Dock, Menu, Progress, RepoTab};
use crate::words;
use components::{badge, button, heading, id, quiet};
use forge::Reply;
use view::design::Ladder;

const DOCK_W: Pixels = px(300.);
pub(crate) const PAGE_X: Pixels = px(20.);
pub(crate) const TAB_BAR_H: Pixels = px(38.);
const FACT_LABEL_W: Pixels = px(120.);

pub(crate) fn render(forge: &mut Forge, cx: &mut Context<Forge>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let mut columns = div()
        .id(id("forge-columns"))
        .flex()
        .size_full()
        .min_h(px(0.));
    if forge.layout.tree_visible() && forge.nav().repo.is_some() {
        columns = columns
            .child(repos::rail(forge, cx, &theme, ladder))
            .child(design::divider(
                id("forge-rail-resize"),
                "Resize the repository list",
                &theme,
                cx,
                |forge: &mut Forge, dx| {
                    forge.layout.tree += dx;
                    forge.layout.clamp();
                },
            ));
    }
    columns = columns.child(main(forge, cx, &theme, ladder));
    if let Some(dock) = forge
        .nav()
        .dock
        .filter(|_| forge.layout.dock_visible() && forge.nav().change.is_none())
    {
        columns = columns.child(panel(forge, dock, cx, &theme, ladder));
    }
    let root = div()
        .id(id("forge"))
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(columns)
        .children(crate::policies::sheet(forge, cx, ladder, &theme));
    let (label, floating) = match forge.menu {
        None => ("", None),
        Some(menu @ (Menu::Ref | Menu::Source | Menu::Target)) => (
            words::PICK_A_REF,
            Some(components::menu(menu, "Refs", forge, cx, &theme, ladder)),
        ),
        Some(menu @ Menu::Head) => (
            "Pick the default head",
            Some(components::menu(
                menu,
                words::DEFAULT_HEAD,
                forge,
                cx,
                &theme,
                ladder,
            )),
        ),
    };
    let mut root = modal_overlay(id("forge-menu-overlay"), label, root, floating);
    if let Some(menu) = forge.menu {
        root = root.on_dismiss(
            cx.listener(move |forge, _: &(), window, cx| forge.close_dropdown(menu, window, cx)),
        );
    }
    root
}

fn main(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let mut column = div()
        .id(id("forge-main"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.));
    if forge.layout.narrow() && forge.nav().repo.is_some() {
        column = column.child(narrow_bar(forge, cx, theme, ladder));
    }
    if !forge.notice.is_empty() {
        column = column.child(
            div()
                .id(id("forge-notice"))
                .m_2()
                .p_2()
                .bg(theme.danger_soft)
                .text_color(theme.foreground)
                .text_size(design::text::SECONDARY)
                .child(forge.notice.clone()),
        );
    }
    if forge.session.connected && forge.me_principal().is_none() {
        column = column.child(no_account(theme));
    }
    let body: AnyElement = match (forge.nav().repo.clone(), forge.nav().change) {
        (None, _) => repos::overview(forge, cx, theme, ladder),
        (Some(_), Some(_)) => change::render(forge, cx, theme, ladder),
        (Some(_), None) => repo(forge, cx, theme, ladder),
    };
    column.child(body).into_any_element()
}

fn no_account(theme: &Theme) -> AnyElement {
    div()
        .id(id("forge-no-account"))
        .m_2()
        .p_3()
        .bg(theme.warning_soft)
        .text_color(theme.muted)
        .text_size(design::text::SECONDARY)
        .child(
            "To create repositories, push, open changes or review, create or join an account in \
             Account. You can read every repository without an account.",
        )
        .into_any_element()
}

fn repo(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let name = forge.repo_name();
    let mut tabs = div()
        .id(id("forge-tabs"))
        .h(TAB_BAR_H)
        .flex()
        .items_center()
        .gap(design::space::XL)
        .px(PAGE_X)
        .border_b_1()
        .border_color(theme.border)
        .child(ref_picker(forge, cx, theme));
    let active = forge.tab_cursor.unwrap_or(forge.nav().tab);
    let at = |tab| {
        RepoTab::ALL
            .iter()
            .position(|it| *it == tab)
            .unwrap_or_default()
    };
    let mut list = design::composite(id("forge-tab-list"), Role::TabList, "Repository")
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(at(active), RepoTab::ALL.len())
        .on_move(cx.processor(|forge, index: usize, _, cx| {
            forge.tab_cursor = Some(RepoTab::ALL[index]);
            cx.notify();
        }))
        .on_press(
            cx.processor(|forge, index: usize, _, cx| forge.open_tab(RepoTab::ALL[index], cx)),
        )
        .build()
        .h_full()
        .flex()
        .items_center()
        .gap(design::space::XL);
    for tab in RepoTab::ALL {
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_tab(tab, cx));
        let count = (tab == RepoTab::Changes)
            .then(|| forge.open_changes())
            .flatten();
        list = list.child(components::tab(
            id(format!("forge-tab-{}", tab.slug())),
            tab.label(),
            count,
            forge.nav().tab == tab,
            active == tab,
            theme,
            ladder,
            open,
        ));
    }
    let about = cx.listener(|forge, _: &ClickEvent, _, cx| forge.toggle_dock(Dock::About, cx));
    tabs = tabs.child(list).child(div().flex_1()).child(
        button(id("forge-dock-about"), "About", ladder, theme, about)
            .selected(forge.nav().dock == Some(Dock::About))
            .kind(design::Kind::Quiet),
    );
    let body: AnyElement = match forge.nav().tab {
        RepoTab::Readme => readme::render(forge, cx, theme, ladder),
        RepoTab::Code => code::render(forge, cx, theme, ladder),
        RepoTab::Commits => commits::render(forge, cx, theme, ladder),
        RepoTab::Changes => changes::render(forge, cx, theme, ladder),
        RepoTab::Refs => refs::render(forge, cx, theme, ladder),
        RepoTab::Settings => settings::render(forge, cx, theme, ladder),
    };
    div()
        .id(id("forge-repo"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(title(forge, &name, cx, theme, ladder))
        .child(tabs)
        .child(body)
        .into_any_element()
}

fn title(
    forge: &Forge,
    name: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let url = forge.repo_link(name);
    let copy = cx.listener({
        let (name, url) = (name.to_owned(), url.clone());
        move |forge, _: &ClickEvent, _, cx| {
            cx.host()
                .notify::<view::methods::ClipboardWrite>(url.clone());
            forge.copied = Some(name.clone());
            cx.notify();
        }
    });
    let copied = forge.copied.as_deref() == Some(name);
    let mut title = div()
        .id(id("forge-repo-header"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::MD)
        .px(PAGE_X)
        .pt(design::space::BLOCK)
        .pb(design::space::XS)
        .child(heading(id("forge-repo-name"), name.to_owned(), 1, theme));
    if forge.archived() {
        title = title.child(badge(
            id("forge-repo-archived"),
            words::ARCHIVED,
            ladder,
            theme.warning,
            theme.warning_soft,
        ));
    }
    if let Some((info, _)) = forge.repo() {
        let owner = forge.principal_name(&info.repo.owner);
        title = title.child(
            div()
                .id(id("forge-repo-owner"))
                .flex()
                .items_center()
                .gap(design::space::XS)
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(format!(
                    "owner {owner} · {} · active",
                    design::plural(info.repo.refs_count, "ref", "refs")
                ))
                .child(block_link(
                    id("forge-repo-activity"),
                    info.repo.last_activity,
                    theme,
                    ladder,
                )),
        );
    }
    title
        .child(div().flex_1())
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .text_color(theme.faint)
                .child(url),
        )
        .child(
            button(
                id("forge-repo-copy"),
                if copied { "Copied" } else { "Copy" },
                ladder,
                theme,
                copy,
            )
            .kind(design::Kind::Quiet),
        )
        .into_any_element()
}

pub(crate) fn block_href(height: u64) -> String {
    format!("duck://explorer/block/{height}")
}

pub(crate) fn block_link(
    id: impl Into<ElementId>,
    height: u64,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let label = words::block(height);
    design::link(id, label.clone(), block_href(height), ladder, theme)
        .aria_label(format!("Open {label} in Explorer"))
}

fn ref_picker(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let head = forge.head_name();
    let label = match forge.refs() {
        Some([]) => words::NO_REFS_YET.to_owned(),
        _ => components::ref_label(&head),
    };
    components::dropdown(
        Menu::Ref.key(),
        label,
        forge.menu == Some(Menu::Ref),
        theme,
        cx.listener(|forge, event: &ClickEvent, window, cx| {
            forge.open_dropdown(Menu::Ref, event, window, cx)
        }),
    )
}

fn narrow_bar(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let tree = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.layout.tree_open = !forge.layout.tree_open;
        cx.notify();
    });
    let details = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.layout.dock_open = !forge.layout.dock_open;
        cx.notify();
    });
    div()
        .id(id("forge-narrow-bar"))
        .flex()
        .gap_1()
        .px_2()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
        .child(
            button(id("forge-toggle-rail"), "Repositories", ladder, theme, tree)
                .kind(design::Kind::Outline)
                .selected(forge.layout.tree_open),
        )
        .when(forge.nav().change.is_some(), |bar| {
            bar.child(
                button(id("forge-toggle-dock"), "Details", ladder, theme, details)
                    .kind(design::Kind::Outline)
                    .selected(forge.layout.dock_open),
            )
        })
        .into_any_element()
}

fn panel(
    forge: &Forge,
    dock: Dock,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let close = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.toggle_dock(dock, cx));
    let body: AnyElement = match dock {
        Dock::About => about(forge, theme),
    };
    div()
        .id(id("forge-dock"))
        .w(DOCK_W)
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .id(id("forge-dock-header"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(heading(id("forge-dock-title"), dock.label(), 2, theme))
                .child(div().flex_1())
                .child(button(
                    id("forge-dock-close"),
                    "Close",
                    ladder,
                    theme,
                    close,
                )),
        )
        .child(
            div()
                .id(id("forge-dock-body"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px_3()
                .pb_3()
                .child(body),
        )
        .into_any_element()
}

fn about(forge: &Forge, theme: &Theme) -> AnyElement {
    let Some((info, bounds)) = forge.repo() else {
        return quiet(words::READING_REPOSITORY, theme);
    };
    let mut column = div()
        .id(id("forge-about"))
        .flex()
        .flex_col()
        .gap_2()
        .child(fact(
            words::DEFAULT_HEAD,
            components::ref_label(&info.repo.settings.head),
            theme,
        ))
        .child(fact("Hash", words::hash_kind(info.repo.hash), theme))
        .child(fact("Owner", forge.principal_name(&info.repo.owner), theme))
        .child(fact(words::PAGE_SIZE, bounds.page_size.to_string(), theme))
        .child(fact(
            words::INLINE_BLOB_BOUND,
            words::bytes(bounds.blob_bytes),
            theme,
        ))
        .child(heading(id("forge-about-access"), "Access", 3, theme));
    let Some(list) = forge.writers() else {
        return column
            .child(quiet(words::READING_WRITERS, theme))
            .into_any_element();
    };
    let writers = Writer::rows(list);
    if writers.is_empty() {
        column = column.child(quiet(words::ONLY_THE_OWNER, theme));
    }
    for writer in &writers {
        column = column.child(quiet(forge.writer_name(writer), theme));
    }
    column.into_any_element()
}

pub(crate) fn fact(label: &str, value: impl Into<String>, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .gap_2()
        .text_size(design::text::SECONDARY)
        .child(
            div()
                .w(FACT_LABEL_W)
                .text_color(theme.muted)
                .child(label.to_owned()),
        )
        .child(div().flex_1().child(value.into()))
        .into_any_element()
}

pub(crate) fn pending(forge: &Forge, scope: &str, theme: &Theme) -> AnyElement {
    let ops = forge.pending_in(scope);
    if ops.is_empty() {
        return div().into_any_element();
    }
    let mut column = div()
        .id(id(format!("forge-pending-{scope}")))
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .py_1();
    for op in ops {
        let (tone, status) = match &op.progress {
            Progress::Submitting => (theme.surface_raised, words::SUBMITTING.to_owned()),
            Progress::Accepted => (theme.surface_raised, words::WAITING_FOR_BLOCK.to_owned()),
            Progress::Refused(answer) => (
                theme.danger_soft,
                design::failure(answer).unwrap_or_default(),
            ),
        };
        column = column.child(
            div()
                .id(id(format!("forge-pending-{}", op.id)))
                .flex()
                .items_center()
                .gap_2()
                .p_1()
                .bg(tone)
                .text_size(design::text::SECONDARY)
                .child(op.label.clone())
                .child(quiet(status, theme)),
        );
    }
    column.into_any_element()
}

pub(crate) fn staged<'a>(
    forge: &'a Forge,
    query: &forge::Query,
    element_id: &str,
    loading_text: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Result<&'a Reply, AnyElement> {
    match forge.stage(query) {
        crate::Stage::Ready(reply) => Ok(reply),
        crate::Stage::Loading => Err(reading(element_id, loading_text, theme)),
        crate::Stage::Failed(refusal) => {
            Err(refused(query, element_id, refusal, cx, theme, ladder))
        }
    }
}

pub(crate) fn reading(element_id: &str, text: &str, theme: &Theme) -> AnyElement {
    components::loading(id(format!("{element_id}-loading")), text, theme)
}

pub(crate) fn refused(
    query: &forge::Query,
    element_id: &str,
    refusal: &view::host::Error,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let query = query.clone();
    let retry = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.retry(query.clone(), cx));
    view::design::refused(element_id, design::sentence(refusal), ladder, theme, retry)
        .m_2()
        .into_any_element()
}

pub(crate) fn scroller(name: &str) -> Stateful<Div> {
    div()
        .id(id(name.to_owned()))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
}

pub(crate) fn bold(text: impl Into<String>) -> AnyElement {
    div()
        .font_weight(FontWeight::MEDIUM)
        .child(text.into())
        .into_any_element()
}
