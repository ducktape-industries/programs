//! What stands where a read has not landed: static shapes in the skeleton's colour, no words and
//! no pictures, laid out as the rows that replace them lay out. The patch that brings the data
//! replaces them; nothing here moves or waits on time.
use super::{Face, Type};
use crate::prelude::*;
use crate::{Div, Hsla, Pixels, Stateful, px};

// gpui-kit's list loading row (`crates/component/src/list/loading.rs`): a title's bar 192 wide
// over a caption's 256 wide, each no wider than its slot, and three such rows.
pub const TITLE: Pixels = px(192.);
pub const CAPTION: Pixels = px(256.);
pub const ROWS: usize = 3;

// A box where something not read yet goes, `fill` the raised tone of the ground it sits on
// (`theme.surface_raised`, gpui-kit's `skeleton.background`; `theme.sidebar_raised` on a
// sidebar). The caller sizes it.
pub fn bar(fill: Hsla) -> Div {
    div().flex_none().bg(fill)
}

// A line of `role` words not read yet: a bar as tall as the role's letters, `width` wide and no
// wider than its slot, centred on the line the words take where the role names one.
pub fn line(role: Type, width: Pixels, fill: Hsla) -> Div {
    let leading = role.line.map_or(px(0.), |line| (line - role.size) / 2.);
    div()
        .w_full()
        .py(leading)
        .child(bar(fill).h(role.size).w(width).max_w_full())
}

// A face not read yet: `face`'s round box.
pub fn face(face: impl Into<Face>, fill: Hsla) -> Div {
    bar(fill).size(face.into().side).rounded_full()
}

// What holds a read's placeholders: one group that says it is busy, named `label`, so a screen
// reader hears what is being read where the eye sees shapes.
pub fn region(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(label)
        .aria_busy(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{POINTER, TOUCH};
    use crate::wire;

    fn lower(element: impl IntoElement) -> (wire::Node, wire::Styles) {
        let (node, styles, _) = crate::testing::lower(|_: &mut Context<()>| element);
        (node, styles)
    }

    fn holds_words(node: &wire::Node) -> bool {
        matches!(node, wire::Node::Text(_)) || node.children().iter().any(holds_words)
    }

    #[test]
    fn a_region_of_placeholders_says_it_is_busy_and_draws_no_words() {
        let theme = Theme::light();
        let fill = theme.surface_raised;
        let rows = (0..ROWS).map(|_| {
            div()
                .flex()
                .child(face(TOUCH.avatar, fill))
                .child(line(TOUCH.body, TITLE, fill))
        });
        let (node, _) = lower(region("loading", "Loading messages…").children(rows));
        let held = node.interactivity().expect("a group");
        assert_eq!(held.role, Some(Role::Group));
        assert_eq!(held.aria.label.as_deref(), Some("Loading messages…"));
        assert!(held.aria.busy);
        assert_eq!(node.children().len(), ROWS);
        assert!(!holds_words(&node), "{node:?}");
        assert!(wire::audit(&node).is_empty());
    }

    #[test]
    fn a_line_is_as_tall_as_its_roles_letters_on_the_roles_line() {
        let theme = Theme::light();
        for ladder in [&TOUCH, &POINTER] {
            let (node, styles) = lower(line(ladder.body, TITLE, theme.surface_raised));
            let leading = ladder
                .body
                .line
                .map_or(px(0.), |line| (line - ladder.body.size) / 2.);
            let slot = &styles[node.style().expect("a styled line")];
            assert_eq!(slot.padding.top, Some(leading.into()));
            assert_eq!(slot.padding.bottom, Some(leading.into()));
            let drawn = &styles[node.children()[0].style().expect("a styled bar")];
            assert_eq!(drawn.size.height, Some(ladder.body.size.into()));
            assert_eq!(drawn.size.width, Some(TITLE.into()));
            assert_eq!(drawn.background, Some(theme.surface_raised.into()));
        }
    }

    #[test]
    fn a_face_is_its_sides_round_box() {
        let theme = Theme::light();
        let (node, styles) = lower(face(TOUCH.avatar, theme.surface_raised));
        let drawn = &styles[node.style().expect("a styled face")];
        let side = Some(TOUCH.avatar.side.into());
        assert_eq!((drawn.size.width, drawn.size.height), (side, side));
        assert!(node.children().is_empty());
    }
}
