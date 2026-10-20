//! A view's tests: [`TestAppContext`] drives one view as the app does, over
//! a [`FakeHost`] that answers its requests by their shape, and holds every
//! frame to the host's rules: its sanitizer, the accessibility audit, the
//! per-frame request budget, and keys delivered only along the focus path.
//! [`Relay`] links two or more of them as one node's live tracks, so what
//! one view publishes the others hear.
//!
//! ```
//! # use serde::{Deserialize, Serialize};
//! # use view::prelude::*;
//! # use view::{View, testing::TestAppContext};
//! #[derive(Default, Serialize, Deserialize)]
//! struct Rows;
//! impl View for Rows {
//!     const NAME: &'static str = "Rows";
//! }
//! impl Render for Rows {
//!     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
//!         uniform_list("rows", 100, |range, _, _| {
//!             range.map(|row| div().id(row).child(format!("row {row}"))).collect()
//!         })
//!     }
//! }
//! let mut cx = TestAppContext::new();
//! cx.open::<Rows>();
//! // the first frame holds a screenful: the rows that fill the pane the
//! // host opened the view in, and a margin past them
//! assert!(cx.texts().len() > 20);
//! // a scroll to rows the window does not hold moves the window to them
//! cx.simulate_range("rows", 80..90);
//! assert!(cx.texts().contains(&"row 89".to_owned()));
//! ```

use crate::wire::{Node, TooltipResponse};

pub(crate) fn texts(root: Option<&Node>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(root) = root {
        collect_texts(root, &mut out);
    }
    out
}

fn collect_texts(node: &Node, out: &mut Vec<String>) {
    match node {
        Node::RichText { text, .. } => out.push(text.clone()),
        Node::Text(crate::wire::TextNode { content, .. }) => out.push(content.clone()),
        Node::Field {
            value: text,
            placeholder,
            ..
        } => out.push(if text.is_empty() {
            placeholder.clone()
        } else {
            text.clone()
        }),
        _ => node
            .children()
            .iter()
            .for_each(|child| collect_texts(child, out)),
    }
}

pub(crate) fn assert_taken_whole(taken: Result<crate::wire::SanitizeReport, crate::wire::Refused>) {
    match taken {
        Err(refused) => panic!("the host refuses this frame: {refused}"),
        Ok(cut) if !cut.is_empty() => {
            panic!("the host would cut this frame, it is past a frame budget: {cut:?}")
        }
        Ok(_) => {}
    }
}

pub(crate) fn assert_frame_accessible(root: Option<&Node>, tooltips: &[TooltipResponse]) {
    let tooltips = tooltips
        .iter()
        .filter_map(|response| response.content.as_deref());
    root.into_iter().chain(tooltips).for_each(assert_accessible);
}

/// Lowers the one element `build` makes under a view's context, with no
/// host behind it: the tree, the table its nodes' styles are in, and the
/// presses [`design::dense`](crate::design::dense) marked in it (what
/// [`assert_fits`] takes beside the two).
/// Nothing holds the tree to the host's rules here, as [`TestAppContext`]
/// holds every frame of a view: this is for a test of what one element
/// carries, [`assert_accessible`] its audit when the test wants one.
pub fn lower<V, E: crate::IntoElement>(
    build: impl FnOnce(&mut crate::Context<V>) -> E,
) -> (
    Node,
    crate::wire::Styles,
    Vec<Vec<crate::wire::ElementIdWire>>,
) {
    let mut app = crate::App::for_driver();
    let entity = crate::Entity::reserve(&app);
    let mut window = app.window();
    let element = build(&mut crate::Context {
        app: &mut app,
        entity,
    });
    let tree = crate::Lowering::new(&mut window, &mut app).lower(element);
    let dense = app.inner.dense.take();
    (tree, app.styles(), dense)
}

/// Panics listing each node assistive technology cannot name, place or
/// reach, by its key path and fault.
pub fn assert_accessible(tree: &Node) {
    let faults = crate::wire::audit(tree);
    assert!(
        faults.is_empty(),
        "{} accessibility fault(s):\n{}",
        faults.len(),
        faults
            .iter()
            .map(|fault| format!("  {:?} at {}", fault.kind, fault.path.join(" > ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

pub(crate) fn chain<'a>(
    root: &'a Node,
    matches: &mut dyn FnMut(&[&'a Node]) -> bool,
) -> Option<Vec<&'a Node>> {
    fn walk<'a>(
        node: &'a Node,
        chain: &mut Vec<&'a Node>,
        matches: &mut dyn FnMut(&[&'a Node]) -> bool,
    ) -> bool {
        chain.push(node);
        if matches(chain)
            || node
                .children()
                .iter()
                .any(|child| walk(child, chain, matches))
        {
            return true;
        }
        chain.pop();
        false
    }
    let mut out = Vec::new();
    walk(root, &mut out, matches).then_some(out)
}

pub(crate) fn chain_to<'a>(root: &'a Node, key: &str) -> Option<Vec<&'a Node>> {
    chain(root, &mut |chain| chain.last().unwrap().key() == Some(key))
}

pub(crate) fn authored_path(chain: &[&Node]) -> Vec<crate::wire::ElementIdWire> {
    let row = |at: usize| {
        let parent = chain[at.checked_sub(1)?];
        let index = parent
            .children()
            .iter()
            .position(|child| std::ptr::eq(child, chain[at]))?;
        crate::wire::identity::row(parent, index)
    };
    (0..chain.len())
        .filter_map(|at| crate::wire::identity::segment(chain[at].identity().cloned(), row(at)))
        .collect()
}

pub(crate) fn pieces(root: &Node) -> Vec<(Vec<crate::wire::ElementIdWire>, &Node)> {
    fn walk<'a>(
        chain: &mut Vec<&'a Node>,
        out: &mut Vec<(Vec<crate::wire::ElementIdWire>, &'a Node)>,
    ) {
        let node = *chain.last().unwrap();
        if matches!(
            node,
            Node::Camera { .. } | Node::Microphone { .. } | Node::Remote { .. }
        ) {
            out.push((authored_path(chain), node));
        }
        for child in node.children() {
            chain.push(child);
            walk(chain, out);
            chain.pop();
        }
    }
    let mut out = Vec::new();
    walk(&mut vec![root], &mut out);
    out
}

pub(crate) fn button<'a>(root: &'a Node, name: &str) -> Option<Vec<&'a Node>> {
    chain(root, &mut |chain| match chain.last().unwrap() {
        node @ Node::Container(crate::wire::ContainerNode {
            interactivity: Some(interactivity),
            ..
        }) if interactivity.on_click.is_some() => {
            let mut labels = Vec::new();
            collect_texts(node, &mut labels);
            node.key() == Some(name)
                || interactivity.aria.label.as_deref() == Some(name)
                || labels.iter().any(|label| label == name)
        }
        _ => false,
    })
}

pub(crate) fn input<'a>(root: &'a Node, name: &str) -> Option<Vec<&'a Node>> {
    chain(root, &mut |chain| match chain.last().unwrap() {
        Node::Field {
            id, placeholder, ..
        } => id.name() == Some(name) || placeholder == name,
        _ => false,
    })
}

pub(crate) fn keys(root: Option<&Node>) -> Vec<String> {
    fn collect(node: &Node, out: &mut Vec<String>) {
        if let Some(key) = node.key() {
            out.push(key.to_string());
        }
        node.children().iter().for_each(|child| collect(child, out));
    }
    let mut out = Vec::new();
    if let Some(root) = root {
        collect(root, &mut out);
    }
    out
}

/// The phone widths a view is swept at (`(width, height)` in logical px):
/// the narrowest, the common and the widest phone.
pub const PHONES: [(f32, f32); 3] = [(360., 780.), (390., 844.), (430., 932.)];

/// The width sweep (SPEC §8.1d-1): the open view is told the device is
/// touch-first, laid out at each of [`PHONES`], and `drive` runs there (the
/// states the view's §10 row lists), with every frame held to
/// [`assert_accessible`] and [`assert_fits`] at that width under the touch
/// ladder; then a resize 390 -> 360 -> 390 must give the 390 layout again
/// (no sticky size state). A view's `tests/sweep.rs` is one call.
pub fn sweep(cx: &mut TestAppContext, mut drive: impl FnMut(&mut TestAppContext)) {
    let ladder = crate::design::ladder(true);
    cx.simulate_touch(true);
    for (width, height) in PHONES {
        cx.fits = Some((width, ladder));
        cx.simulate_resize(width, height);
        drive(cx);
    }
    let (narrow, common) = (PHONES[0], PHONES[1]);
    cx.fits = Some((common.0, ladder));
    cx.simulate_resize(common.0, common.1);
    let before = cx.root().clone();
    cx.fits = Some((narrow.0, ladder));
    cx.simulate_resize(narrow.0, narrow.1);
    cx.fits = Some((common.0, ladder));
    cx.simulate_resize(common.0, common.1);
    assert_eq!(
        cx.root(),
        &before,
        "a resize {} -> {} -> {} did not give the {} layout again: size state is sticky",
        common.0,
        narrow.0,
        common.0,
        common.0
    );
    cx.fits = None;
}

enum Press {
    Declared(f32),
    Reaches(f32),
    Undeclared,
    ReachesPastAnUndeclaredParent,
}

/// Panics listing each node that does not fit a body `width` wide under
/// `ladder`: a fixed or minimum width past `width`; a pressable node (a
/// click route, a field) whose height is under `ladder.hit`, or that is
/// invisible. Its height is its own declared one, else the nearest row
/// above it that declares one; for a node pinned absolute to its parent's
/// top and bottom, that row's less the two insets. A node that reaches
/// past its parent (negative insets, `design::hit`) is measured off its
/// parent alone, which must declare its height: the reach is sized to
/// that parent, so a taller row further up says nothing about the box.
/// An absolute box is placed inside its parent's border, so it is the
/// parent's height less the border, and the reach.
/// Also a many-line field with an `on_submit` (the software Return is a
/// newline); the root's `on_back` not exactly when there is somewhere to
/// go back to (a sheet open, or a Back control drawn: a node named "Back",
/// as `design::page` names it for [`Nav::Back`](crate::design::Nav::Back)).
/// A long press has no wire signal of its own, so its custom-action twin
/// (rule 5) is the review's.
///
/// It reads declared sizes only: the harness has no layout engine, so a
/// width or height left to layout is not judged here (the qa phone-layout
/// goldens are). So it trusts a press box under 44 when the box takes its
/// height from a declared row above it that is 44 or more:
/// - a `design::hit` told its parent is 44 or more each way, so it
///   reaches nowhere;
/// - a box pinned by insets that add up to zero (`top(-12)`,
///   `bottom(12)`);
/// - a box pinned by one inset only;
/// - a press with no declared height of its own.
///
/// And it refuses a `design::hit` whose parent's height is left to
/// layout (`h_full`, `size_full`) even where the box fits.
///
/// A press filed under one of the paths in `dense` (the presses
/// [`design::dense`](crate::design::dense) marked as `tree` lowered) may be
/// under a finger's height: a dense list keeps its row's own press box, as
/// the Expo app's does. A press inside it is held to the height as any.
pub fn assert_fits(
    tree: &Node,
    styles: &crate::wire::Styles,
    dense: &[Vec<crate::wire::ElementIdWire>],
    width: f32,
    ladder: &crate::design::Ladder,
) {
    let mut faults = Vec::new();
    let mut sheet_open = false;
    let mut back_drawn = false;
    let hit: f32 = ladder.hit.into();
    let px_of = |length: &Option<gpui::Length>| match length {
        Some(gpui::Length::Definite(gpui::DefiniteLength::Absolute(
            gpui::AbsoluteLength::Pixels(px),
        ))) => Some(f32::from(*px)),
        _ => None,
    };
    let declared = |id: Option<crate::wire::StyleId>| id.and_then(|id| styles.get(id));
    let height_of = |node: &Node| {
        let style = declared(node.style())?;
        px_of(&style.size.height).or_else(|| px_of(&style.min_size.height))
    };
    let border_of = |node: &Node| {
        let edge = |width: Option<gpui::AbsoluteLength>| match width {
            Some(gpui::AbsoluteLength::Pixels(px)) => f32::from(px),
            _ => 0.,
        };
        declared(node.style()).map_or(0., |style| {
            edge(style.border_widths.top) + edge(style.border_widths.bottom)
        })
    };
    let pinned = |node: &Node| {
        let style = declared(node.style())?;
        if style.position != Some(gpui::Position::Absolute) {
            return None;
        }
        Some(px_of(&style.inset.top)? + px_of(&style.inset.bottom)?)
    };
    let press_height = |chain: &[&Node]| {
        let node = *chain.last().expect("a chain ends at its node");
        if let Some(own) = height_of(node) {
            return Press::Declared(own);
        }
        let reach = pinned(node).filter(|insets| *insets < 0.).map(f32::abs);
        if let Some(reach) = reach {
            let parent = chain.iter().rev().nth(1).copied();
            let inside = parent.and_then(|parent| Some(height_of(parent)? - border_of(parent)));
            return inside.map_or(Press::ReachesPastAnUndeclaredParent, |inside| {
                Press::Reaches(inside + reach)
            });
        }
        let height = chain
            .iter()
            .rev()
            .skip(1)
            .find_map(|node| height_of(node))
            .map(|above| above - pinned(node).unwrap_or(0.));
        height.map_or(Press::Undeclared, Press::Declared)
    };
    chain(tree, &mut |chain| {
        let node = *chain.last().unwrap();
        let path = || {
            chain
                .iter()
                .filter_map(|node| node.key().map(str::to_owned))
                .collect::<Vec<_>>()
                .join(" > ")
        };
        if let Some(style) = declared(node.style()) {
            for (what, length) in [
                ("width", &style.size.width),
                ("min width", &style.min_size.width),
            ] {
                if let Some(px) = px_of(length)
                    && px > width
                {
                    faults.push(format!("{what} {px} past the body's {width} at {}", path()));
                }
            }
        }
        let pressable = matches!(node, Node::Field { .. })
            || node.interactivity().is_some_and(|i| i.on_click.is_some());
        if pressable {
            let invisible = declared(node.style())
                .is_some_and(|style| style.visibility == Some(gpui::Visibility::Hidden));
            if invisible {
                faults.push(format!("a pressable node is invisible at {}", path()));
            }
            let marked = dense.contains(&authored_path(chain));
            match press_height(chain) {
                _ if marked => {}
                Press::Declared(height) | Press::Reaches(height) if height >= hit => {}
                Press::Reaches(height) => faults.push(format!(
                    "a press box reaching past its parent comes to {height} (the parent inside its border, and the reach), under {hit}, at {}: tell `design::hit` the parent's true size and border (`edge`)",
                    path()
                )),
                Press::ReachesPastAnUndeclaredParent => faults.push(format!(
                    "a press box reaches past a parent that declares no height at {}: declare the parent's height (`.h(..)` or `.min_h(..)`); one left to layout (`h_full`, `size_full`) is not read",
                    path()
                )),
                Press::Declared(_) | Press::Undeclared => faults.push(format!(
                    "a pressable node declares no height of at least {hit} (itself or the nearest row above it that declares one) at {}",
                    path()
                )),
            }
        }
        if let Node::Field {
            multiline: true,
            on_submit: Some(_),
            ..
        } = node
        {
            faults.push(format!("a many-line field submits on Return at {}", path()));
        }
        sheet_open |= matches!(node, Node::Sheet { .. });
        back_drawn |= node
            .interactivity()
            .is_some_and(|i| i.aria.label.as_deref() == Some("Back"));
        false
    });
    let somewhere_back = sheet_open || back_drawn;
    let on_back = tree.interactivity().and_then(|i| i.on_back).is_some();
    if somewhere_back && !on_back {
        faults.push(match sheet_open {
            true => "a sheet is open and the root has no on_back".into(),
            false => "a Back control is drawn and the root has no on_back".into(),
        });
    }
    if on_back && !somewhere_back {
        faults.push("the root has on_back with no sheet open and no Back control drawn: the phone's Back would pop nothing".into());
    }
    assert!(
        faults.is_empty(),
        "{} node(s) do not fit {width} px under touch:\n{}",
        faults.len(),
        faults
            .iter()
            .map(|fault| format!("  {fault}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

mod context;
mod fake_host;
mod focus;
mod notifier;
mod relay;
pub use context::{TestAppContext, TickReport, VIEWPORT};

/// What a test host answers an `op.submit` it took: [`Submitted::Applied`]
/// with `output`, under a frame identity no test reads.
pub fn applied(output: Vec<u8>) -> crate::methods::Submitted {
    crate::methods::Submitted::Applied {
        frame: [0; 32],
        until: 0,
        output,
    }
}
pub use fake_host::{FakeHost, StreamSender};
pub use notifier::TestNotifier;
pub use relay::Relay;

#[cfg(test)]
pub(crate) struct Probe;
#[cfg(test)]
impl crate::methods::Program for Probe {
    const NAME: &'static str = "probe";
    type Op = ();
    type Query = ();
    type Reply = ();

    fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
        unreachable!()
    }

    fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
        unreachable!()
    }
}
