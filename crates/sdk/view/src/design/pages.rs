//! The one route model a view has at every width: its [`Pages`] stack,
//! and the [`page`] frame one page is drawn in under a Compact body.
use serde::{Deserialize, Serialize};

use super::{Ladder, TextType, heading, space};
use crate::interactivity::EventListener;
use crate::prelude::*;
use crate::{AnyElement, Div, FontWeight, Hsla, Stateful};
use gpui::SharedString;

/// The view's own route stack, newest last: part of the view's state, so
/// it snapshots and restores with it. One stack drives every width: a
/// Compact body shows [`top`](Self::top) alone and Back pops; a wider body
/// reads the same `top()` into its panes, so a resize across the Compact
/// edge loses nothing. gpui's own pattern (a view owns its state and
/// renders it), not a router.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(deserialize = "P: Deserialize<'de>"))]
pub struct Pages<P> {
    /// Never empty: a restore of an empty stack is refused, as `top()`
    /// has nothing to answer and `depth()` nothing to count from.
    #[serde(deserialize_with = "rooted")]
    stack: Vec<P>,
}

/// A stack at the page's own default: a view's state derives `Default`.
impl<P: Default> Default for Pages<P> {
    fn default() -> Self {
        Self::root(P::default())
    }
}

fn rooted<'de, P: Deserialize<'de>, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<P>, D::Error> {
    let stack = Vec::<P>::deserialize(deserializer)?;
    match stack.is_empty() {
        true => Err(serde::de::Error::custom("a page stack has its root")),
        false => Ok(stack),
    }
}

impl<P> Pages<P> {
    /// A stack with `page` at its root.
    pub fn root(page: P) -> Self {
        Self { stack: vec![page] }
    }
    pub fn push(&mut self, page: P) {
        self.stack.push(page);
    }
    /// The top page, popped; never the root, which stays and answers
    /// `None`. A Back that arrives a tick late (JSC lands a frame behind)
    /// is therefore a no-op at the root.
    pub fn pop(&mut self) -> Option<P> {
        (self.stack.len() > 1).then(|| self.stack.pop()).flatten()
    }
    /// Replaces the top page: a list's selection moving while a detail is
    /// open.
    pub fn replace(&mut self, page: P) {
        *self.stack.last_mut().expect("a stack has its root") = page;
    }
    pub fn top(&self) -> &P {
        self.stack.last().expect("a stack has its root")
    }
    /// The root page, under every other.
    pub fn first(&self) -> &P {
        self.stack.first().expect("a stack has its root")
    }
    /// `0` at the root: `.when(pages.depth() > 0, |root| root.on_back(..))`
    /// sets Back exactly when there is somewhere to go back to.
    pub fn depth(&self) -> usize {
        self.stack.len() - 1
    }
    /// Root first.
    pub fn iter(&self) -> impl Iterator<Item = &P> {
        self.stack.iter()
    }
}

/// What the title row keeps clear on the right, under touch, for the
/// capsule the host floats over a running program (the owner's call of
/// 2026-10-15: a capsule only, no host bar; the view's own title row draws
/// Back).
pub const CAPSULE_CLEARANCE: Pixels = px(96.);

/// The key of one part of page `id`: `{id}-{part}`.
fn named(id: &ElementId, part: &str) -> ElementId {
    ElementId::Name(format!("{id}-{part}").into())
}

/// Where a [`page`] sits in its view's stack, said at every [`page`]
/// call, so a first page cannot be drawn as a child page by leaving
/// something out. Chosen from the stack's depth:
/// `match pages.depth() { 0 => Nav::root(reader), depth => Nav::back(depth, pop) }`.
///
/// Under a finger the depth crosses with the page (the root's is `0`), so
/// the host moves from one page to the next as the phone does: a deeper
/// page slides in over the one it was opened from, a shallower one is
/// uncovered, and a page at the same depth (a tab) is put in place. The
/// view draws only the page on top and writes nothing for the motion.
pub enum Nav {
    /// The view's first page and the reader on it (the Expo kit's
    /// `Header` `root={{ name }}`, `Header.tsx:19-23`). Under a finger its
    /// title is [`Ladder::big_title`], its hairline clear (`Header.tsx:41`,
    /// `:59`), and the reader's avatar leads the row (`{id}-account`,
    /// [`Ladder::avatar`] in a [`Ladder::hit`] box, named "Account", no
    /// dim on a press, as Expo's `Pressable` at `Header.tsx:46-54` sets
    /// none). A press asks the host to bring up the reader's account
    /// (`host.show` [`Place::Account`], so the view declares
    /// `Capability::Host`) as a `notify`: a refusal (no account to show, a
    /// host older than `host.show`) is silent and the press does nothing.
    /// No other page draws the avatar. A pointer draws the first page as
    /// a [`Nav::Child`], with no avatar.
    ///
    /// The name is the reader's account name, read as the view reads any
    /// account's name (the session names only the account number). While
    /// it is still being read it is empty, and the avatar draws `•`
    /// ([`initial`](super::initial)).
    Root(SharedString),
    /// A page under another, and the way back to it: a control
    /// (`{id}-back`, [`Ladder::hit`] each way, named "Back") that runs the
    /// listener. Under a finger it is the Expo kit's header `Hit` with the
    /// Lucide arrow (`Header.tsx:45`); under a pointer a `‹` at the title
    /// size. The phone's own Back is not this: it is the view root's
    /// `on_back` (SPEC §9 rule 13), set from the same depth. The depth is
    /// the page's place over the root: `1` for a page opened from the
    /// root, one more for each page over that, a page the view lays over
    /// its stack (a form, a link's empty page) one more than the stack's
    /// top.
    Back(usize, EventListener<ClickEvent>),
    /// A page under another with no Back of its own (the Expo `Header`
    /// with neither `back` nor `root`, `Header.tsx:16`, `:23`): the 17
    /// title over the hairline. It is on no stack the view moves, so it
    /// crosses with no depth and the host never slides it.
    Child,
}

impl Nav {
    /// [`Nav::Root`] with the reader's account name.
    pub fn root(reader: impl Into<SharedString>) -> Self {
        Self::Root(reader.into())
    }
    /// [`Nav::Back`] at `depth` over the root, running `back`.
    pub fn back(depth: usize, back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        Self::Back(depth, Box::new(back))
    }
}

/// One page's frame: a title row (`{id}-title`, [`Ladder::header`] tall)
/// with the heading (`{id}-heading`, truncating), its Back or the reader's
/// avatar by `nav`, what the page adds beside it ([`Page::leading`],
/// [`Page::caption`], [`Page::trailing`]), over the children, which fill
/// the rest: `design::page("rooms", top, match pages.depth() { 0 =>
/// Nav::root(reader), depth => Nav::back(depth, pop) }, &ladder, &theme)`.
///
/// Under a finger the row is the Expo kit's `Header` (`Header.tsx:28-73`):
/// 56 tall, 8 in, 4 between its parts, a hairline under it, the title in
/// [`Ladder::title`] medium.
pub fn page(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    nav: Nav,
    ladder: &Ladder,
    theme: &Theme,
) -> Page {
    let id = id.into();
    let (root, depth, nav) = match nav {
        Nav::Root(reader) => (
            true,
            Some(0),
            ladder.touch.then(|| account(&id, &reader, ladder, theme)),
        ),
        Nav::Back(depth, back) => (
            false,
            Some(u32::try_from(depth).unwrap_or(u32::MAX)),
            Some(back_control(&id, back, ladder, theme)),
        ),
        Nav::Child => (false, None, None),
    };
    Page {
        id,
        root,
        depth,
        title: title.into(),
        caption: None,
        nav,
        leading: Vec::new(),
        trailing: Vec::new(),
        ladder: *ladder,
        theme: *theme,
        body: Vec::new(),
    }
}

/// The reader's avatar on a first page under a finger ([`Nav::Root`]).
fn account(id: &ElementId, reader: &str, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let show = |_: &ClickEvent, _: &mut Window, cx: &mut App| {
        cx.host().notify::<HostShow>(Place::Account);
    };
    div()
        .id(named(id, "account"))
        .size(ladder.hit)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .role(Role::Button)
        .aria_label("Account")
        .focusable()
        .on_click(show)
        .child(super::avatar(reader, ladder.avatar, theme))
        .into_any_element()
}

/// A page's Back control ([`Nav::Back`]).
fn back_control(
    id: &ElementId,
    back: EventListener<ClickEvent>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let theme = *theme;
    let id = named(id, "back");
    match ladder.touch {
        true => super::icon_button(
            id,
            super::icon("arrow-left", ladder.icon),
            "Back",
            ladder,
            &theme,
            back,
        )
        .into_any_element(),
        false => div()
            .id(id)
            .min_w(ladder.hit)
            .min_h(ladder.hit)
            .flex()
            .items_center()
            .justify_center()
            .text_type(ladder.title)
            .text_color(theme.muted)
            .cursor_pointer()
            .hover(move |style| style.text_color(theme.foreground))
            .role(Role::Button)
            .aria_label("Back")
            .focusable()
            .on_click(back)
            .child("‹")
            .into_any_element(),
    }
}

/// A [`page`] being filled: its children go under the title row, in a
/// column that takes the rest of the page's height. The title row is
/// built when the page is drawn, so its slots can land after [`page`]:
/// its Back or the reader's avatar and [`leading`](Self::leading) first,
/// the words
/// (the heading over a [`caption`](Self::caption)) taking the slack and
/// truncating, then [`trailing`](Self::trailing), before what the row
/// keeps clear for the capsule under touch.
pub struct Page {
    id: ElementId,
    /// the view's first page ([`Nav::Root`])
    root: bool,
    /// its place over the root, from its [`Nav`]; none for [`Nav::Child`]
    depth: Option<u32>,
    title: SharedString,
    caption: Option<SharedString>,
    /// Back, or the reader's avatar on a first page under a finger
    nav: Option<AnyElement>,
    leading: Vec<AnyElement>,
    trailing: Vec<AnyElement>,
    ladder: Ladder,
    theme: Theme,
    body: Vec<AnyElement>,
}

impl Page {
    /// A line under the title (`{id}-caption`, [`Ladder::caption`] size,
    /// truncating): a count, a state, who the page is with.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }
    /// An element before the heading, after Back: another person's avatar
    /// (a DM room's peer).
    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading.push(element.into_any_element());
        self
    }
    /// A control of the page itself at the row's end (New, Copy, Edit,
    /// a search, ‹ ›), sized by the caller to the ladder.
    ///
    /// Under a finger the host's capsule sits just past it: `ellipsis`
    /// named More (the program's menu), then `x` named Close. A control
    /// here must not read as the capsule's More: it draws what is behind
    /// it (`plus` where everything behind it creates, `pencil` where
    /// everything edits, `ellipsis-vertical` for a mix), never `ellipsis`,
    /// and is named for what it does (Add, Actions), never More or Close.
    pub fn trailing(mut self, control: impl IntoElement) -> Self {
        self.trailing.push(control.into_any_element());
        self
    }
}

impl gpui::prelude::FluentBuilder for Page {}

impl ParentElement for Page {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl IntoElement for Page {
    type Element = Stateful<Div>;
    fn into_element(self) -> Self::Element {
        let (ladder, theme, id, root) = (self.ladder, self.theme, self.id, self.root);
        let title = match (ladder.touch, root) {
            (true, true) => ladder.big_title,
            _ => ladder.title,
        };
        let words = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                heading(named(&id, "heading"), self.title, 1, &theme)
                    .truncate()
                    .text_type(title)
                    .when(ladder.touch, |title| title.font_weight(FontWeight::MEDIUM)),
            )
            .children(self.caption.map(|caption| {
                div()
                    .id(named(&id, "caption"))
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .truncate()
                    .child(caption)
            }));
        let title_row = div()
            .id(named(&id, "title"))
            .flex_none()
            .min_h(ladder.header)
            .flex()
            .items_center()
            .map(|row| match ladder.touch {
                // its own band (`Header.tsx:33-43`): the body scrolls under
                // it, never through it. A root's hairline is drawn clear
                // (`Header.tsx:40-41`), so the row is one height on every
                // page and the title does not move when the stack deepens.
                true => row
                    .gap(space::XXS)
                    .px(space::SM)
                    .pr(CAPSULE_CLEARANCE)
                    .bg(theme.background)
                    .border_b_1()
                    .border_color(match root {
                        true => Hsla::transparent_black(),
                        false => theme.border,
                    }),
                false => row.gap(space::SM).px(space::LG),
            })
            .children(self.nav)
            .map(|row| match ladder.touch {
                // the kit's title block (`Header.tsx:57`): 8 in, an avatar 8
                // from the words
                true => row.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(space::SM)
                        .px(space::SM)
                        .children(self.leading)
                        .child(words),
                ),
                false => row.children(self.leading).child(words),
            })
            .children(self.trailing);
        let mut page = div().id(id);
        // under a finger the host moves between pages by their depth; a
        // pointer's frames carry none and keep their bytes
        if ladder.touch
            && let Some(depth) = self.depth
        {
            page.interactivity().interactions().page = Some(depth);
        }
        page.flex().flex_col().size_full().child(title_row).child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .children(self.body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_never_pops_its_root() {
        let mut pages = Pages::root("rooms");
        assert_eq!(pages.depth(), 0);
        assert_eq!(pages.pop(), None);
        pages.push("room");
        pages.push("thread");
        assert_eq!(pages.depth(), 2);
        assert_eq!(pages.top(), &"thread");
        assert_eq!(pages.pop(), Some("thread"));
        pages.replace("details");
        assert_eq!(
            pages.iter().copied().collect::<Vec<_>>(),
            ["rooms", "details"]
        );
        assert_eq!(pages.pop(), Some("details"));
        assert_eq!(pages.pop(), None);
        assert_eq!(pages.top(), &"rooms");
        let bytes = serde_json::to_string(&pages).unwrap();
        assert_eq!(serde_json::from_str::<Pages<&str>>(&bytes).unwrap(), pages);
        assert!(serde_json::from_str::<Pages<&str>>(r#"{"stack":[]}"#).is_err());
    }
}
