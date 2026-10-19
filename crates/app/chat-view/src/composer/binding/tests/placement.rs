use super::*;
use view::StyleRefinement;

fn roster_of(count: usize) -> Vec<MentionChoice> {
    (0..count)
        .map(|n| MentionChoice {
            token: format!("<@{n}>"),
            label: format!("user{n:04}"),
            hint: None,
            offered: true,
            sigil: '@',
        })
        .collect()
}

fn typed_at(composer: Place) -> Draft {
    let mut draft = caret("@", 1);
    draft.placed = Some(composer);
    draft
}

fn lowered_in(draft: &Draft, choices: &[MentionChoice], pane: Place) -> (wire::Node, wire::Styles) {
    let (tree, styles, _) = testing::lower(|cx| {
        view(
            draft,
            "c",
            "New message",
            "Message #general",
            "Send",
            None,
            true,
            choices,
            Some(pane),
            cx,
            |_: &mut ComposerView, _, _, _| {},
        )
    });
    (tree, styles)
}

fn opened_at(root: &wire::Node) -> Option<(wire::Anchor, Option<[f32; 2]>)> {
    let mut found = None;
    walk(root, &mut |node| {
        if let wire::Node::Anchored {
            anchor, position, ..
        } = node
        {
            found = Some((*anchor, *position));
        }
    });
    found
}

fn menu_style(root: &wire::Node, styles: &wire::Styles) -> StyleRefinement {
    let frame = node(root, "c/mention-menu").expect("the menu is open");
    styles[frame.style().expect("a styled frame")].clone()
}

fn margin() -> f32 {
    f32::from(design::space::SM)
}

#[test]
fn the_composer_hears_where_it_is() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<ComposerView>();
    cx.simulate_bounds("c", (20., 480.), (760., 100.));
    view.read(|view| assert_eq!(view.draft.placed, Some(COMPOSER)));
}

#[test]
fn many_accounts_scroll_inside_the_room_above_the_composer() {
    let choices = roster_of(2_000);
    let (root, styles) = lowered_in(&typed_at(COMPOSER), &choices, PANE);
    let room = COMPOSER.y - PANE.y - margin();
    let frame = menu_style(&root, &styles);
    assert_eq!(frame.max_size.height, Some(px(room).into()));
    assert_eq!(frame.size.width, Some(px(COMPOSER.width).into()));
    assert_eq!(
        opened_at(&root),
        Some((wire::Anchor::BottomLeft, Some([COMPOSER.x, COMPOSER.y])))
    );
    let Some(wire::Node::UniformList {
        count, children, ..
    }) = node(&root, "c/mentions")
    else {
        panic!("the matches are a list")
    };
    assert_eq!(*count, choices.len(), "no match is left out");
    assert!(
        children.len() < choices.len(),
        "only the rows around what shows are drawn: {}",
        children.len()
    );
}

#[test]
fn the_arrows_reach_the_last_match_and_bring_it_into_view() {
    let choices = roster_of(300);
    let last = choices.len() - 1;
    let mut draft = typed_at(COMPOSER);
    let press = |draft: &mut Draft, keystroke: &str| {
        let tag = draft
            .key_tag(&key(keystroke), false, &choices)
            .expect("the open menu claims the arrows");
        draft.act(&tag, "c/editor", &choices);
    };
    for _ in 0..=last {
        press(&mut draft, "down");
    }
    assert_eq!(draft.menu_index, last, "past the last, down stays there");
    let selected = |draft: &Draft, index: usize| {
        let (root, _) = lowered_in(draft, &choices, PANE);
        let key = format!("c/mention/<@{index}>");
        let Some(wire::Node::Container(wire::ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) = node(&root, &key)
        else {
            panic!("{key} is not drawn")
        };
        interactivity.aria.selected
    };
    assert_eq!(selected(&draft, last), Some(true));
    press(&mut draft, "up");
    assert_eq!(draft.menu_index, last - 1);
    assert_eq!(selected(&draft, last - 1), Some(true));
}

#[test]
fn the_menu_opens_on_the_side_with_more_room() {
    let pane = Place {
        x: 0.,
        y: 0.,
        width: 400.,
        height: 300.,
    };
    let choices = roster_of(50);
    let low = Place {
        x: 10.,
        y: 200.,
        width: 380.,
        height: 90.,
    };
    let (root, styles) = lowered_in(&typed_at(low), &choices, pane);
    assert_eq!(
        opened_at(&root),
        Some((wire::Anchor::BottomLeft, Some([10., 200.])))
    );
    let above = px(200. - margin()).into();
    assert_eq!(menu_style(&root, &styles).max_size.height, Some(above));
    let high = Place { y: 10., ..low };
    let (root, styles) = lowered_in(&typed_at(high), &choices, pane);
    assert_eq!(
        opened_at(&root),
        Some((wire::Anchor::TopLeft, Some([10., 100.])))
    );
    let below = px(300. - 100. - margin()).into();
    assert_eq!(menu_style(&root, &styles).max_size.height, Some(below));
}

#[test]
fn the_menu_waits_for_its_place() {
    let mut draft = typed_at(COMPOSER);
    draft.placed = None;
    let (root, _) = lowered_in(&draft, &roster_of(3), PANE);
    assert!(node(&root, "c/mention-menu").is_none());
    let squeezed = Place {
        height: PANE.height,
        ..PANE
    };
    let (root, _) = lowered_in(&typed_at(squeezed), &roster_of(3), PANE);
    assert!(
        node(&root, "c/mention-menu").is_none(),
        "no room on either side"
    );
}
