//! Event routes and sent pictures belong to one running driver.
use std::any::Any;
use std::cell::RefCell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use gpui::Image;

use crate::wire::ElementIdWire;

type TooltipHandler =
    Rc<dyn Fn(Option<usize>, &mut crate::Window, &mut crate::App) -> Option<crate::AnyView>>;

/// What a tooltip route builds: the content, or `None` from an
/// index-sensitive builder with nothing to show there.
struct TooltipRoute(TooltipHandler);
pub(crate) type TooltipBuilder =
    Box<dyn Fn(&mut crate::Window, &mut crate::App) -> crate::AnyView + 'static>;
pub(crate) type RichTextTooltipBuilder =
    Box<dyn Fn(usize, &mut crate::Window, &mut crate::App) -> Option<crate::AnyView> + 'static>;
type EventHandler<A> = Rc<dyn Fn(&A, &mut crate::Window, &mut crate::App)>;

struct EventRoute<A>(EventHandler<A>);

#[derive(Default)]
struct Tables {
    host: crate::Host,
    routes: Routes,
    tooltip_responses: Vec<crate::wire::TooltipResponse>,
    pictures: HashSet<u64>,
    /// each image drawn, by its address: the key its picture is sent by
    images: HashMap<usize, (Weak<Image>, u64)>,
    /// Widget commands asked this tick, sent with its frame
    /// ([`crate::window::send_widgets`]).
    widgets: Vec<crate::wire::WidgetCommand>,
}

/// What a listener is for: the field of the node it lowers into. A node
/// takes one route per field, so its routes never renumber each other; a
/// mouse button is inside the field (`on_mouse_down` runs for every button
/// it was given) and a dispatch phase is a field of its own (`capture_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Click,
    AuxClick,
    MouseDown,
    CaptureMouseDown,
    MouseDownOut,
    MouseUp,
    CaptureMouseUp,
    MouseUpOut,
    MousePressure,
    CaptureMousePressure,
    MouseMove,
    MouseExit,
    ScrollWheel,
    Pinch,
    CapturePinch,
    KeyDown,
    CaptureKeyDown,
    KeyUp,
    CaptureKeyUp,
    ModifiersChanged,
    Hover,
    FileDropExit,
    FileDrop,
    Tooltip,
    Action(gpui::accesskit::Action),
    Change,
    Key,
    Submit,
    Bounds,
    WebState,
    MediaState,
    Drag,
    Dismiss,
    Back,
    ListRequest,
    ListScroll,
    RichClick,
    RichHover,
    RichTooltip,
}

/// Where a listener was authored: the scope of the element that carries it
/// (an id-less element's is its nearest identified ancestor's), what it is
/// for, and which one of that kind in that scope it is; a listener inside
/// a tooltip's content names the tooltip's own route too, since the content
/// lowers in a scope of its own and lives as long as that route.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    within: Option<u32>,
    scope: u32,
    kind: Kind,
    ordinal: u32,
}

/// A scope: an identified element, named by its segment under the scope
/// around it. Its number stands for its whole path, since the scope around
/// it is a number that stands for the path so far; [`ROOT`] is around the
/// outermost. A scope is looked up by these three, exactly: two paths
/// never share a number.
#[derive(Clone, PartialEq, Eq, Hash)]
struct ScopeKey {
    within: Option<u32>,
    parent: u32,
    segment: ElementIdWire,
}

/// The scope of a lowering itself, around every identified element in it.
const ROOT: u32 = 0;

/// A scope being lowered: its segment until a listener in it or under it
/// asks for a route, then its number. A scope no listener asks in is never
/// looked up.
enum Open {
    Unasked(ElementIdWire),
    Numbered(u32),
}

struct Slot {
    id: u32,
    /// The frame that last lowered the key.
    seen: u64,
}

/// The route table: what the host's `handler` numbers mean. An id is
/// handed out the first time its key is seen and kept while the key is
/// lowered, so a route the host took from the frame it painted names the
/// same listener in the next frame, whatever changed around it. A frame
/// lowered without the key frees the id, and no id is handed out twice, so
/// a freed route names nothing for the rest of the driver's life.
///
/// A scope's number is kept the same way: handed out the first time a
/// listener is lowered in it or under it, kept while one is, freed by a
/// frame that lowers none there, and never handed out twice. So a key,
/// which names its scope by number, is the same key for as long as the
/// path it was authored under is.
#[derive(Default)]
struct Routes {
    ids: HashMap<Key, Slot>,
    live: HashMap<u32, Rc<dyn Any>>,
    next: u32,
    frame: u64,
    within: Option<u32>,
    scopes: HashMap<ScopeKey, Slot>,
    /// The last scope number handed out; [`ROOT`] is never handed out.
    last_scope: u32,
    /// The scopes being lowered, outermost first.
    open: Vec<Open>,
    /// `(kind, taken)` for the scopes being lowered, innermost last;
    /// `marks` says where each scope's counters start.
    counters: Vec<(Kind, u32)>,
    marks: Vec<usize>,
    // The journals of the memos being lowered, innermost last.
    journals: Vec<Journal>,
}

// What a memo's lowering took from the route table: the keys and the
// scopes it looked up, how many scopes were open around it, and the number
// of the one it sat in, once it took a route.
#[derive(Default)]
pub(crate) struct Journal {
    keys: Vec<Key>,
    scopes: Vec<ScopeKey>,
    depth: usize,
    base: Option<u32>,
}

impl Routes {
    /// The number of the scope being lowered, innermost: every scope
    /// around it that has none yet is looked up on the way, outermost
    /// first, each under the number of the one around it.
    fn scope(&mut self) -> u32 {
        let numbered = self
            .open
            .iter()
            .rposition(|scope| matches!(scope, Open::Numbered(_)));
        let mut parent = match numbered.map(|at| &self.open[at]) {
            Some(Open::Numbered(number)) => *number,
            _ => ROOT,
        };
        for at in numbered.map_or(0, |at| at + 1)..self.open.len() {
            let Open::Unasked(segment) = std::mem::replace(&mut self.open[at], Open::Numbered(0))
            else {
                unreachable!("every scope past the last numbered one is unasked")
            };
            let key = ScopeKey {
                within: self.within,
                parent,
                segment,
            };
            for journal in &mut self.journals {
                journal.scopes.push(key.clone());
            }
            parent = match self.scopes.entry(key) {
                Entry::Occupied(mut slot) => {
                    slot.get_mut().seen = self.frame;
                    slot.get().id
                }
                Entry::Vacant(slot) => {
                    self.last_scope = self
                        .last_scope
                        .checked_add(1)
                        .expect("scope numbers exhausted");
                    slot.insert(Slot {
                        id: self.last_scope,
                        seen: self.frame,
                    });
                    self.last_scope
                }
            };
            self.open[at] = Open::Numbered(parent);
        }
        parent
    }

    fn take(&mut self, kind: Kind, route: Rc<dyn Any>) -> u32 {
        let start = self.marks.last().copied().unwrap_or(0);
        let ordinal = match self.counters[start..]
            .iter_mut()
            .find(|(taken, _)| *taken == kind)
        {
            Some((_, taken)) => {
                *taken += 1;
                *taken - 1
            }
            None => {
                self.counters.push((kind, 1));
                0
            }
        };
        let key = Key {
            within: self.within,
            scope: self.scope(),
            kind,
            ordinal,
        };
        for journal in &mut self.journals {
            journal.keys.push(key);
        }
        let id = match self.ids.entry(key) {
            Entry::Occupied(mut slot) => {
                slot.get_mut().seen = self.frame;
                slot.get().id
            }
            Entry::Vacant(slot) => {
                let id = self.next;
                self.next = id.checked_add(1).expect("route ids exhausted");
                slot.insert(Slot {
                    id,
                    seen: self.frame,
                });
                id
            }
        };
        self.live.insert(id, route);
        id
    }

    // A memo starts lowering: the routes it takes are written down, so a
    // later frame that keeps it keeps them too.
    fn begin_journal(&mut self) {
        self.journals.push(Journal {
            depth: self.open.len(),
            ..Journal::default()
        });
    }

    fn end_journal(&mut self) -> Journal {
        let mut journal = self.journals.pop().expect("a journal begun");
        if !journal.keys.is_empty() {
            journal.base = Some(match journal.depth.checked_sub(1) {
                None => ROOT,
                Some(at) => match self.open.get(at) {
                    Some(Open::Numbered(number)) => *number,
                    _ => unreachable!("a route numbers every scope around it"),
                },
            });
        }
        journal
    }

    // A kept memo takes its routes again, as the frame that lowered it took
    // them: the same ids for the same listeners. False, and nothing taken,
    // when one of them is gone or the scope around it is another.
    fn replay(&mut self, journal: &Journal) -> bool {
        let Some(base) = journal.base else {
            return true;
        };
        if self.open.len() != journal.depth || self.scope() != base {
            return false;
        }
        let held = journal.keys.iter().all(|key| self.ids.contains_key(key))
            && journal
                .scopes
                .iter()
                .all(|key| self.scopes.contains_key(key));
        if !held {
            return false;
        }
        let frame = self.frame;
        for key in &journal.keys {
            if let Some(slot) = self.ids.get_mut(key) {
                slot.seen = frame;
            }
        }
        for key in &journal.scopes {
            if let Some(slot) = self.scopes.get_mut(key) {
                slot.seen = frame;
            }
        }
        for outer in &mut self.journals {
            outer.keys.extend_from_slice(&journal.keys);
            outer.scopes.extend(journal.scopes.iter().cloned());
        }
        true
    }

    fn get(&self, id: u32) -> Option<Rc<dyn Any>> {
        self.live.get(&id).cloned()
    }

    fn begin_lowering(&mut self, within: Option<u32>) {
        self.within = within;
        self.open.clear();
        self.counters.clear();
        self.marks.clear();
    }

    fn enter_scope(&mut self, segment: ElementIdWire) {
        self.open.push(Open::Unasked(segment));
        self.marks.push(self.counters.len());
    }

    fn leave_scope(&mut self) {
        self.open.pop();
        if let Some(mark) = self.marks.pop() {
            self.counters.truncate(mark);
        }
    }

    /// Frees every key and every scope the frame did not lower. A route
    /// authored inside a tooltip's content is lowered once, when the host
    /// asks for that tooltip, and lives as long as the tooltip's own route
    /// does; so does the scope it was authored in.
    fn end_frame(&mut self) {
        let frame = self.frame;
        let kept: HashSet<u32> = self
            .ids
            .values()
            .filter(|slot| slot.seen == frame)
            .map(|slot| slot.id)
            .collect();
        let alive = |seen: u64, within: Option<u32>| {
            seen == frame || within.is_some_and(|owner| kept.contains(&owner))
        };
        let live = &mut self.live;
        self.ids.retain(|key, slot| {
            let alive = alive(slot.seen, key.within);
            if !alive {
                live.remove(&slot.id);
            }
            alive
        });
        self.scopes.retain(|key, slot| alive(slot.seen, key.within));
    }
}

#[derive(Clone, Default)]
pub struct Context(Rc<RefCell<Tables>>);

impl Context {
    pub(crate) fn with_host(host: crate::Host) -> Self {
        let context = Self::default();
        context.0.borrow_mut().host = host;
        context
    }
}

pub fn picture(context: &Context, bytes: impl AsRef<[u8]>) -> (u64, Option<Vec<u8>>) {
    let bytes = bytes.as_ref();
    send(context, hash(bytes), bytes)
}

/// An image's picture, keyed by its bytes' hash taken the first frame it
/// draws and kept while the image lives: a frame that draws it again
/// neither hashes nor copies its bytes. (gpui's own `Image::id` is a
/// `usize` hash, 32 bits on wasm32: too few to name a picture by.)
pub fn image(context: &Context, image: &Arc<Image>) -> (u64, Option<Vec<u8>>) {
    let at = Arc::as_ptr(image) as usize;
    let kept = context.0.borrow().images.get(&at).map(|(_, key)| *key);
    let key = kept.unwrap_or_else(|| {
        let key = hash(image.bytes());
        let images = &mut context.0.borrow_mut().images;
        // a kept `Weak` holds its image's allocation, so no other image is
        // made at that address while its entry stays
        images.retain(|_, (kept, _)| kept.strong_count() > 0);
        images.insert(at, (Arc::downgrade(image), key));
        key
    });
    send(context, key, image.bytes())
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// `key`'s bytes, the first time the host is sent them.
fn send(context: &Context, key: u64, bytes: &[u8]) -> (u64, Option<Vec<u8>>) {
    let tables = &mut *context.0.borrow_mut();
    if !tables.pictures.insert(key) {
        return (key, None);
    }
    (key, Some(bytes.to_vec()))
}

pub(crate) fn clear_pictures(context: &Context) {
    context.0.borrow_mut().pictures.clear();
}

/// A lowering begins: its routes are keyed in the tree, or inside the
/// tooltip whose content it lowers.
pub(crate) fn begin_lowering(context: &Context, within: Option<u32>) {
    context.0.borrow_mut().routes.begin_lowering(within);
}

/// An identified element starts lowering, filed under `segment` in the
/// scope around it: its listeners are the first of their kind in its scope.
pub(crate) fn enter_scope(context: &Context, segment: ElementIdWire) {
    context.0.borrow_mut().routes.enter_scope(segment);
}

pub(crate) fn leave_scope(context: &Context) {
    context.0.borrow_mut().routes.leave_scope();
}

pub(crate) fn begin_journal(context: &Context) {
    context.0.borrow_mut().routes.begin_journal();
}

pub(crate) fn end_journal(context: &Context) -> Journal {
    context.0.borrow_mut().routes.end_journal()
}

pub(crate) fn replay(context: &Context, journal: &Journal) -> bool {
    context.0.borrow_mut().routes.replay(journal)
}

/// The root is about to lower a frame.
pub(crate) fn begin_frame(context: &Context) {
    context.0.borrow_mut().routes.frame += 1;
}

/// The frame is lowered: every route it did not take is freed.
pub(crate) fn end_frame(context: &Context) {
    context.0.borrow_mut().routes.end_frame();
}

pub(crate) fn tooltip(context: &Context, build: TooltipBuilder) -> u32 {
    let route: Rc<dyn Any> = Rc::new(TooltipRoute(Rc::new(move |_, window, cx| {
        Some(build(window, cx))
    })));
    context.0.borrow_mut().routes.take(Kind::Tooltip, route)
}

pub(crate) fn rich_text_tooltip(context: &Context, build: RichTextTooltipBuilder) -> u32 {
    let route: Rc<dyn Any> = Rc::new(TooltipRoute(Rc::new(move |index, window, cx| {
        index.and_then(|index| build(index, window, cx))
    })));
    context.0.borrow_mut().routes.take(Kind::RichTooltip, route)
}

/// Builds the tooltip route `request` names, if it names one.
pub(crate) fn build_tooltip(
    context: &Context,
    request: u32,
    character_index: Option<usize>,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> Option<Option<crate::AnyView>> {
    let route = context.0.borrow().routes.get(request)?;
    let route = route.downcast_ref::<TooltipRoute>()?.0.clone();
    Some(route(character_index, window, app))
}

pub(crate) fn tooltip_response(context: &Context, response: crate::wire::TooltipResponse) {
    context.0.borrow_mut().tooltip_responses.push(response);
}

pub(crate) fn take_tooltip_responses(context: &Context) -> Vec<crate::wire::TooltipResponse> {
    std::mem::take(&mut context.0.borrow_mut().tooltip_responses)
}

pub(crate) fn route<A: 'static>(
    context: &Context,
    kind: Kind,
    listener: impl Fn(&A, &mut crate::Window, &mut crate::App) + 'static,
) -> u32 {
    let route: Rc<dyn Any> = Rc::new(EventRoute::<A>(Rc::new(listener)));
    context.0.borrow_mut().routes.take(kind, route)
}

/// Runs the route `index` names, if it names one that takes an `A`.
pub(crate) fn run_route<A: 'static>(
    context: &Context,
    index: u32,
    event: &A,
    window: &mut crate::Window,
    app: &mut crate::App,
) -> bool {
    let handler = context.0.borrow().routes.get(index);
    let Some(route) =
        handler.and_then(|route| route.downcast_ref::<EventRoute<A>>().map(|r| r.0.clone()))
    else {
        return false;
    };
    route(event, window, app);
    true
}

pub(crate) fn host(context: &Context) -> crate::Host {
    context.0.borrow().host.clone()
}

pub(crate) fn widget(context: &Context, command: crate::wire::WidgetCommand) {
    context.0.borrow_mut().widgets.push(command);
}

pub(crate) fn take_widgets(context: &Context) -> Vec<crate::wire::WidgetCommand> {
    std::mem::take(&mut context.0.borrow_mut().widgets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_contexts_keep_their_own_routes_and_picture_history() {
        let first = Context::default();
        let first_route = route::<String>(&first, Kind::Change, |_, _, _| {});
        assert!(picture(&first, b"svg").1.is_some());
        {
            let second = Context::default();
            let route = route::<String>(&second, Kind::Change, |_, _, _| {});
            assert_eq!(
                (first_route, route),
                (0, 0),
                "each driver starts its own route table"
            );
            assert!(
                picture(&second, b"svg").1.is_some(),
                "a new host needs its own picture bytes"
            );
        }
        assert!(
            picture(&first, b"svg").1.is_none(),
            "returning to the first driver preserves its picture history"
        );
    }

    /// A frame that lowers one listener in each scope `paths` names;
    /// answers the scope number each was keyed under.
    fn frame(routes: &mut Routes, paths: &[&[&'static str]]) -> Vec<u32> {
        routes.frame += 1;
        routes.begin_lowering(None);
        let scopes = paths
            .iter()
            .map(|path| {
                for segment in *path {
                    routes.enter_scope(ElementIdWire::Name((*segment).into()));
                }
                let scope = routes.scope();
                routes.take(Kind::Click, Rc::new(()));
                path.iter().for_each(|_| routes.leave_scope());
                scope
            })
            .collect();
        routes.end_frame();
        scopes
    }

    /// A scope's number stands for its whole path: one segment under two
    /// parents is two scopes, and a scope keeps its number for as long as
    /// a frame lowers a listener in it, whatever is lowered around it.
    #[test]
    fn a_scope_number_names_one_path_and_is_kept_across_frames() {
        let mut routes = Routes::default();
        let first = frame(&mut routes, &[&["a", "row"], &["b", "row"], &["row"]]);
        let [a_row, b_row, row] = first[..] else {
            unreachable!()
        };
        assert!(a_row != b_row && a_row != row && b_row != row, "{first:?}");
        assert!(!first.contains(&ROOT), "the root is no element's scope");

        // the same paths, lowered in another order beside a new one
        let second = frame(&mut routes, &[&["row"], &["c", "row"], &["b", "row"]]);
        assert_eq!((second[0], second[2]), (row, b_row));
        assert!(!first.contains(&second[1]), "a new path is a new scope");

        // `a/row` was not lowered: its number is gone for good
        let third = frame(&mut routes, &[&["a", "row"], &["b", "row"]]);
        assert_eq!(third[1], b_row);
        assert!(!first.contains(&third[0]) && !second.contains(&third[0]));
    }

    /// A scope no listener is lowered in, or under, is never looked up.
    #[test]
    fn a_scope_without_a_listener_is_not_numbered() {
        let mut routes = Routes::default();
        routes.frame += 1;
        routes.begin_lowering(None);
        routes.enter_scope(ElementIdWire::Name("page".into()));
        routes.enter_scope(ElementIdWire::Name("plain".into()));
        routes.leave_scope();
        routes.enter_scope(ElementIdWire::Name("button".into()));
        routes.take(Kind::Click, Rc::new(()));
        routes.leave_scope();
        routes.leave_scope();
        let mut numbered: Vec<_> = routes
            .scopes
            .keys()
            .map(|key| key.segment.name().unwrap().to_owned())
            .collect();
        numbered.sort();
        assert_eq!(numbered, ["button", "page"]);
    }

    // A memo's routes, taken again from its journal, keep their ids through
    // a frame that does not lower them, and go with the first frame that
    // neither lowers nor keeps it.
    #[test]
    fn a_journal_keeps_the_routes_a_memo_took() {
        let mut routes = Routes::default();
        let page = || ElementIdWire::Name("page".into());
        routes.frame += 1;
        routes.begin_lowering(None);
        routes.enter_scope(page());
        routes.begin_journal();
        routes.enter_scope(ElementIdWire::Name("row".into()));
        let id = routes.take(Kind::Click, Rc::new(()));
        routes.leave_scope();
        let journal = routes.end_journal();
        routes.leave_scope();
        routes.end_frame();

        routes.frame += 1;
        routes.begin_lowering(None);
        routes.enter_scope(page());
        assert!(routes.replay(&journal));
        routes.leave_scope();
        routes.end_frame();
        assert!(routes.get(id).is_some(), "the kept route lives on");

        routes.frame += 1;
        routes.begin_lowering(None);
        routes.enter_scope(ElementIdWire::Name("elsewhere".into()));
        assert!(!routes.replay(&journal), "another scope is another place");
        routes.leave_scope();
        routes.end_frame();
        assert!(
            routes.get(id).is_none(),
            "a frame without the memo frees it"
        );
    }

    #[test]
    fn clearing_picture_history_resends_content() {
        let context = Context::default();
        assert!(picture(&context, b"image").1.is_some());
        assert!(picture(&context, b"image").1.is_none());
        clear_pictures(&context);
        assert_eq!(
            picture(&context, b"image").1.as_deref(),
            Some(b"image".as_slice())
        );
    }
}
