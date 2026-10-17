# Writing a view

A view is a wasm32 cdylib on `view`. It builds a widget tree the host
lays out and draws, hears meaning-level events back, and asks the host for
data through a fixed table of methods. This page is the whole surface.

Read `crates/app/example-view/src/lib.rs` first: one file, the smallest view
that uses every core feature once (a stream followed, a listing read, a
bound field, a write, a refusal shown, a restore), with a comment on every
piece and its tests under it. It is built and tested with the workspace and
ships in no network. This page is what that file leans on.

## Depending on it

A view crate outside this repository, with this repository checked out
beside it as `programs`, has this `Cargo.toml`:

```toml
[package]
name = "hello-view"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
view = { path = "../programs/crates/sdk/view" }
serde = { version = "1", features = ["derive"] }

[patch.crates-io]
rmp-serde = { git = "https://github.com/ducktape-industries/gpui-pre", rev = "c698ed2c7e8eeed46641e5ea9622ee55b84b1486" }
gpui-pre-scheduler = { git = "https://github.com/ducktape-industries/gpui-pre", rev = "c698ed2c7e8eeed46641e5ea9622ee55b84b1486" }
```

The `[patch.crates-io]` lines are this workspace's own, at the rev its
`Cargo.toml` pins: the SDK builds against the gpui fork's copies of those
two crates (crates.io's `rmp-serde` has no `typed` feature, and without
the fork's scheduler a view does not build for wasm32), and cargo reads
`[patch]` only from the manifest being built, so a crate outside the
workspace repeats them. A view inside the workspace
writes `view.workspace = true` and no patch.

The smallest view, `src/lib.rs`:

```rust
use view::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Hello;

impl View for Hello {
    const NAME: &'static str = "hello";
}

impl Render for Hello {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("hello")
    }
}

export_view!(Hello);
```

`prelude` is every SDK name a view file writes, in one import: gpui's names
(`Render`, `Window`, `Context`, `IntoElement`, `div`, ...), the view's own
(`View`, `Loadable`, `Task`, `Host`, `Error`, `TextField`, `Theme`,
`export_view!`), the `design` module, and the `methods` module whole
(`HostSession`, `Changes`, `Submit`, `Capability`, `Session`, ...). A
program's types come from the program's crate, and an explicit import wins
over the glob (`use forge::Query;`). `View` asks for `Default`, `Serialize`
and `Deserialize`, which is why `serde` is a dependency. It builds with
`cargo build --target wasm32-unknown-unknown`. `crate-type = ["cdylib"]` is
all a view needs: no crate links a view as a library, and its unit tests
run without `rlib`.

## What a view is made of

What the six first-party views write, by how many of them write it. Start
with these; the rest of this page is reference.

| A view writes | Views | What it is |
|---|---|---|
| `impl View`: `NAME`, `CAPABILITIES`, `TARGETS`, `attach` | 6/6 | What the host reads before the view runs, and where what it follows starts ("The `View` trait"). |
| `cx.follow::<D>(request, \|view, item, cx\| ..).detach()` | 6/6 | A host stream: the session, a program's blocks, the clock. One call a stream, in `attach`. |
| `cx.log_refused(what, &refusal)` | 6/6 | A refusal nothing on screen waits for, to the host's log under the view's `NAME`. It asks `host.log`, so the view declares `Capability::Host`. |
| `Loadable<T>`; `cx.load(self, work, \|view\| &mut view.slot)` | 6/6; 5/6 | An answer and its states, and the read that fills it. |
| `cx.land(work, \|view, answer, cx\| ..)` | 6/6 | The answer to your own closure: a write, or a read that does more than fill a slot. |
| `host.query(ask)`, `host.query_all(\|after\| ask)` | 5/6 | A typed question to a program: one page, or the listing whole. |
| `cx.notify()` | 6/6 | The view decides when it renders ("When a view renders"). |
| `cx.listener(\|view, event, window, cx\| ..)` | 6/6 | A method of the view as a handler: a press (`&ClickEvent`), Enter in a field (`&()`), a key. gpui's. |
| `let theme = *cx.global::<Theme>();` | 6/6 | The host's colors, light or dark: gpui's `cx.theme()`. The first line of a render; `design`'s functions take `&theme`. |
| `design::space`, `design::size`, `design::text` | 6/6 | The spacing, control and type tokens every screen is laid out in. |
| `design::empty_state(id, glyph, title, detail, &ladder, &theme)` | 6/6 | What a list says when it has no rows; under a finger the Expo kit's, the Lucide `glyph` over the words. |
| `design::badge(..)` | 6/6 | A small labelled state: "agent", "suspended". |
| `design::refused(name, message, &ladder, &theme, retry)` | 5/6 | A refused read: the reason and a Retry, under the ids `<name>-refused` and `<name>-retry`. |
| `design::composite(..)` | 5/6 | A group one Tab stop holds and the arrows move in: a tab bar, a list of rows. |
| `TextField` and `Input::new(id, &self.field, label)` | 5/6 | A text field bound to the view's state ("What is gpui and what is ours"). |
| `.focusable()`, `.role(..)`, `.aria_disabled(..)` | 5/6 | What a pressable `div` owes the keyboard and a screen reader; the test host fails a frame that lacks one. |
| `cx.processor(\|view, item, window, cx\| ..)` | 5/6 | `cx.listener` for a callback that returns a value: a list's row builder. gpui's. |
| `design::button(id, label, &ladder, &theme, click).enabled(..)` | 3/6 | A button with its role, Tab stop and disabled state done; `.busy(..)` while its write is in flight, `.small()` for the Expo kit's small one. |
| `cx.new(..)`, child entities | 1/6 | State and a render of its own inside a view ("Child entities"). |
| `design::menu(id, label, at, &theme)` of `design::menu_item(..)`s, the modal of a `modal_overlay(..).on_dismiss(..)` | 1/6 | A menu at the pointer: a right press's (`on_mouse_down(MouseButton::Right, ..)`, its `position`), or the Menu key's on a focused row. |

A phone page is built from the `design` kit: each piece draws as the Expo
app's kit does under a finger and as the desktop always has under a
pointer. Pick the ladder once per render, then hand it to every piece.

| A phone page writes | What it is |
|---|---|
| `let ladder = design::ladder(window.touch());` | The sizes for this device: the Expo app's under a finger, the desktop's under a pointer. Every `design` piece takes `&ladder`. |
| `design::row(id, &ladder, &theme)` with `design::row_words(design::row_title(..)).child(design::row_caption(..))`, `design::lead(..)`, `design::chevron(..)` | A list row and its words; a row with a caption is `.min_h(ladder.row_tall)`. |
| `design::field(Input::new(..), &ladder, &theme).label(..).caption(..)` | A text field in its box; `.problem(..)` marks it invalid and says why, `.lead(glyph)` puts a glyph before the text. |
| `design::sheet(..)` holding `design::sheet_title(..)`, `design::sheet_row(..)`, `design::form_verbs(cancel, verb)` | A sheet, its head, its actions (`.icon`, `.hint`, `.danger`) and a form's Cancel and verb. |
| `design::strip(composite, &ladder, &theme)` of `design::tab(..)`s | A bar of tabs under a page head. |
| `design::dense(row)` | A dense list's row (a chat message, a diff line) keeps its own height; the sweep lets it through. |
| `design::sentence(&refusal)`, `design::failure(&answer)` | What a refusal and a write that did not land say, in the same words on every page. |

Three patterns the views share:

- **Who is reading.** `cx.follow::<HostSession>` hands a `Session` per
  change; `account` is `None` both before the host resolved it and for a key
  that holds none. A view tells them apart by what it shows: its slot stays
  `Idle` until the first session item, and a read for no account answers an
  empty list. What a session change means is each view's (chat reopens,
  settings resets a form), so the view compares and decides.
- **A program's type in the state.** The snapshot is serde; a program's
  type may derive borsh only (identity's `Account` holds abi types that do).
  A view keeps a row struct of what the screen shows, folds the program's
  answer into it, and saves a borsh-only field as its bytes with
  `#[serde(with = "view::borsh_bytes")]`. The fold also keeps
  the snapshot to what is drawn.
- **A write.** `host.ask::<Submit<P>>(op)` sends the op; `cx.land` hands the
  answer to a closure that clears the form or keeps the program's reason.
  The view keeps the task (`Option<Task<()>>` under `#[serde(skip)]`) for as
  long as the button reads busy.

## What is gpui and what is ours

The rule: we re-implement only what touches the host or the wire; everything
else is the gpui fork `Cargo.toml` pins (`gpui-pre`, at the rev it names),
re-exported unchanged. The `pub use gpui::{…}` at the top of `src/lib.rs`
is the gpui list (`px`, `rems`, `Hsla`, `StyleRefinement`, `Styled`,
`ElementId`, `SharedString`, the `*Event` types, `Role`, ...).
`use view::prelude::*` brings in what a gpui view file
imports: elements, styles, events and the traits whose methods they call.
A `Styled` method's style crosses whole as the node's style entry, but for
gpui's debug outlines (a debug build's alone) and a pattern background (the
host refuses it). Text styling cascades to the text inside, as in gpui:
colour, family, features, fallbacks, size, line height, weight, slant,
background, underline, strikethrough, white space, overflow, alignment, line
clamp and letter spacing (`.letter_spacing(px(0.66))`, in pixels, negative
to tighten).
Ours, defined in this crate:

- `App`, `AppContext`, `AsyncApp`, `Context`, `Entity`, `WeakEntity`
  (`src/context.rs`), `Task` (`src/executor.rs`), `Window`
  (`src/window.rs`): the entity graph and the tick loop run inside the
  view, so the host never sees a closure. Its `EventEmitter` and
  `Subscription` are gpui's.
- `div`, `uniform_list`, `list`, `Input`, `Textarea`, `img`/`Img`, `svg`/`Svg`,
  `canvas`/`Canvas`, `InteractiveText`/`StyledText`, `sensor`,
  `resize_handle`, `modal_overlay`, `web`, `camera`, `microphone`,
  `remote_video`, `remote_audio` (`src/element.rs`, `src/list.rs`,
  `src/primitives/`, `src/rich_text.rs`, `src/behavior.rs`, `src/web.rs`,
  `src/media.rs`):
  each lowers to a `view_wire::Node`, with handlers kept view-side and
  crossed as routes: a listener's number, kept while its element is lowered
  (`view_wire::Event`). `list` is gpui's but for its first argument:
  styled, with nothing to press or hover (a `div` around it carries those),
  and `list(id, state, ..)` takes an id where gpui's `list(state, ..)` takes
  none, since the host files a list's state and its rows by path (below);
  `canvas` is drawn by calls (`rect`, `circle`, `line`), since gpui's paint
  closures cannot cross.
- `sensor(id, child).on_bounds(listener)` tells a view where layout put
  `child`. gpui hands an element its bounds in its prepaint and paint
  closures, which cannot cross to a view, so the host tells them as an
  event: the listener (`Fn(&Bounds<Pixels>, &mut Window, &mut App)`) hears
  the child's bounds when it comes into view, and whenever its origin or
  its size differs from the last heard while it is in view. They are in the
  window's pixels, as a mouse event's `position` is, so
  `event.position - bounds.origin` is the pointer inside the child: which
  row, which candle, which price. Before the first event the host has not
  laid the view out, and the view draws without them. The listener is told
  once for every frame in which the child's bounds differ from the last
  told, so a sensor inside something that scrolls, or in a pane being
  dragged, hears once a frame while it moves; a listener that stores the
  bounds and does not notify costs an unchanged tick each time.
- An id is unique among the ids under its nearest identified ancestor, as
  gpui's are (`view_wire::identity`). A `list`, a `uniform_list` and each of
  their rows are scopes of their own: a list is filed under the id its
  author gives it (`list(id, state, ..)`, `uniform_list(id, ..)`), a row
  under its own id, else its index, and the ids inside it under the row, so
  rows need no index formatted into their ids. The host keeps what it holds
  for a view by that path (a list's scroll, a field's text and selection,
  focus), so a list's id is what brings the state in its rows back when the
  view is instantiated again, and what a widget target is named through
  (`focus_path(["thread", "rows", key, "edit"])`). Children are not scopes:
  a row of data built with `children(..)` carries its own id (its item's
  key). Two rows named by one key, two lists given one id, or any id written
  twice in a scope are refused by the host, naming the id and its scope; a
  view's test fails in the same words, since `TestAppContext` holds every
  frame to the host's sanitizer.
- A text field is bound from birth: `Input::new(id, &self.name, label)` and
  `Textarea::new(id, &self.body, label)` take the `TextField` of the view
  they show. The host owns the editing; what is typed lands in the field
  (read it with `self.name.text()`) and renders the view, with no listener
  written for it. `.on_change(cx.listener(..))` is for a view that does more
  on a change, and runs after the field took it. A `TextField` is a handle,
  as gpui's `Entity<Editor>` is: a `Clone` of it is the same field (so
  `vec![Form::default(); n]` is one field `n` times), while equality and the
  snapshot are by value. `reset(text)` starts a new document.
  A view that builds its own editor on a field has the rest: `tokens()` and
  `reset_with_tokens(text, tokens)` for its atomic spans, `generation()` and
  `apply(&change)` for the host's word, and `Window::dispatch(command)` to
  send what `replace` and `replace_all` ask.
- A control is named from birth: `Input::new(id, &field, label)`,
  `Textarea::new(id, &field, label)`, `modal_overlay(id, label, …)`, and
  `design`'s `segmented`, `icon_button` and `divider` take the words
  assistive technology reads; the audit catches one given none.
- `InteractiveElement`, `StatefulInteractiveElement`, `FocusHandle`,
  `ScrollHandle` (`src/interactivity.rs`): listeners and focus become wire
  routes; `track_scroll` lets a handle move its scroller. `Window::focus(id)`
  names an element by its own id, and the SDK sends the host the whole path
  the frame it renders gives it (`focus_path` when two scopes hold the id);
  `focus_next`/`focus_prev` move as Tab does.

## The methods

`view-wire/src/methods.rs` is the one list of what a view may ask for
(`methods::ALL`): each kind a marker type naming its request and reply (borsh both ways;
`host.widget` alone is MessagePack, because it names tree ids). The trait is
sealed, so a view cannot invent a kind. A view speaks to the host in four
ways:

```rust
// a stream: an item per change, for as long as the view runs
cx.follow::<Changes<valset::Valset>>((), |view, change, cx| ..).detach();
// one question, one answer
let status = cx.host().ask::<ChainStatus>(()).await?;
// a question to a program, typed by its reply
let accounts = cx.host().query(identity::ask::List { page }).await?;
// something said, with no answer waited for
cx.host().notify::<HostBadge>(3);
```

`cx.follow` is `host.subscribe::<D>(request)`, the raw stream, driven for
the view; a view that drives a stream itself (`cx.spawn`) asks for the
stream.

A program's query is asked alone, typed by the reply that answers it:
`cx.host().query(identity::ask::List { page })` is `program.query` with the
program's own bytes (`identity::Query::List`), answered with the page of
accounts and nothing else. A program's `Query` derives `program::Ask`, which
writes those types into its `ask` module; a reply to another question is
refused as `unexpected_reply`, so a view has no arm for the others.

A node program is addressed by its own type, the one that implements
`program::Program` (`methods::Program` here):
`Query<P>`, `Submit<P>` and `Changes<P>` are its three methods, and
`P::NAME` is the target they carry. A `Changes<P>` item is the block's
`Change`: its height and the keys it wrote; `change.touches::<P, _>(&query)`
says whether a read moved, from the tables the program's `Query` declares
with `#[reads(..)]` (`program::Reads`; another program's tables it reads are
declared with that program named first), so a view re-reads what the block
touched and maps no key itself (`None` is a reopened link: read everything
again). Every refusal is the program SDK's `Error`
(`code` token, `message`), one type end to end: a program's codes are
`error::code`, the host's own are `methods::refusal`. `Loadable<T>` + `cx.load`
(`src/root.rs`) hold an ask's states and snapshot `Loading`
as `Idle`.

A cursored listing is read one of two ways. `host.query_all(|after| ask)`
follows it to its end: the closure is handed each page's cursor (`None`
first) and puts it in the question, and the rows of every page come back as
one list. It is for a list that is whole by nature (a roster, a settings
list), which a screen searches, counts or draws all of, and for an ask
answered with a `PageResponse`; `host::all_pages` is the same walk over a
page-asking closure of your own, for a listing in another shape. `Paged<T>` (`src/paged.rs`) holds it
a page at a time: for a history. It is an entity built from the same
page-asking closure; the `uniform_list` that draws it hands it the rows it
lowers (`show`), and the next page is asked for when they reach past the rows
held, one read out at a time. The list has `count()` rows: the rows held and,
while the listing goes on, one more that the view draws as loading. When a
`Change` touches the listing, `reread` reads the pages held again from the
first (cursors do not outlive a write), not the whole listing. Dropping the
entity cancels the read on its way. Both follow one rule for a cursor the
program refuses `stale` (it was handed out before a write): the read starts
over from the first page, and the refusal is never shown. Any other refusal
is the answer.

`Session` (`methods.rs`, `cx.follow::<HostSession>`) is what every view is handed:
`connected`, `chain_id`, `signer` (the seated key, hex), `account` (its
account number, `None` until the host resolves one), `endpoint`; an item per
change. Read "who am I" from `account`; no view asks identity for it.

## Live tracks

A view that declares `Capability::Live` publishes and subscribes to tracks
in its program's rooms, through the connected node and off consensus: the
four `live.*` methods, and two helpers over them in `src/live.rs`.
`Track::publish(cx, LivePublish { track, mode, priority }, on)` owns a
publication (dropping it unpublishes) and numbers what it sends (`send` and
`next_group` in stream mode, `send_latest` in datagram mode);
`live::subscribe(cx, LiveSubscribe { track, from }, each)` decodes what a
track carries; who is in a room is `cx.follow::<LiveTracks>(room, ..)`.
Build them in `attach`, as any subscription.

The node lets an account into a room only as the view's program granted
it, in rows the program writes (`program::live::grant`) and the node reads
from blocks. An ask the rows do not cover yet hears `Waiting` and stays
open; the node answers it on the block that writes the row. A view that
opens a room submits the op writing the row on each `Waiting` (a
reconnect that still waits tells it again):

```rust
let mine = LivePublish { track, mode: LiveMode::Stream, priority: DEFAULT_PRIORITY };
self.mine = Some(Track::publish(cx, mine, |view, item, cx| match item {
    // the view's own: submits its program's op that writes the row
    Ok(Publishing::Waiting) => view.join(cx),
    Ok(Publishing::Subscribers(count)) => view.listening = count,
    // the host's queue to the node was full: a full state in a new group
    Ok(Publishing::Cut { .. }) => view.keyframe(cx),
    Err(refusal) => view.refused = Some(refusal),
}));
```

`not_granted` is this account's grant taken back: the request's last item;
a subscription whose publisher lost its grant ends `publisher_gone`. The codes
each method ends with are in its doc in `methods.rs`.

A call's media never crosses the view wire: the view places pieces and the
host captures, encodes, publishes, subscribes, decodes, draws and plays
(`src/media.rs`). `camera(id).publish(track)` and
`microphone(id).publish(track).muted(muted)` publish what the device
captures while the piece is placed (the host picks the mode and priority by
the kind); `remote_video(id, track)` draws a person's picture and
`remote_audio(id, track)` plays their voice (zero-size, out of the layout's
flow), each subscribed while placed. A publishing piece needs
`Capability::Live` beside its device's (`camera`, `microphone`); a remote
piece has no device, and needs `live` alone. A piece's `MediaState::track`
says where its track stands (`Waiting`, `Up { subscribers }`,
`Ended { code }`), and a view answers its `Waiting` as it answers a
`live.publish`'s: with the op that writes the row.

## When a view renders, and who decides

A view renders on the tick after it called `cx.notify()`, once however many
times it called it, and at no other time but its first frame and a host that
lost its tree. The view decides: nothing in the SDK notifies on its behalf
for something only the view can judge.

- `cx.follow::<D>(request, each)` subscribes to the host stream `D` and runs
  `each` per item, a refused item included, and does not notify: `each` calls
  `cx.notify()` when the item changed what the view shows. An item that only
  starts a read draws nothing; the read draws when it lands. The `Task` it
  returns is the subscription, as gpui's `Subscription` is: `.detach()` it to
  follow for as long as the view runs, or keep it and drop it to stop.
  `cx.log_refused(what, &refusal)` writes a refusal nothing on screen waits
  for to the host's log, under the view's `NAME`.
- `cx.load(self, work, |view| &mut view.slot)` reads `work` into a `Loadable`
  slot. What the slot shows stays until the answer lands: a value stays on
  screen (`Loadable::Reloading`), a slot with nothing to show (`Idle`,
  `Failed`) shows `Loading`. It notifies when, and only when, it changes what
  the slot shows: a read that lands the value already there draws nothing. A
  read that must blank what is shown first says so in its own line,
  `self.slot = Loadable::Idle;`. The read lives in the slot: replacing or
  dropping the slot cancels it, so a newer read supersedes an older one.
- `cx.land(work, |view, answer, cx| ..)` hands the answer, the value or the
  refusal, to the closure and does nothing else; the closure notifies if it
  moved anything. It is for a write, or a read that does more than fill a
  slot: in gpui's words, `cx.spawn(async move |this, cx| { let answer =
  work.await; this.update(cx, |view, cx| ..) })` in one line. `.detach()` the
  task it returns, or keep it beside what it reads: a newer one stored in its
  place cancels the older.
- A list the host scrolls renders when it needs rows it does not hold.
- A `Paged` notifies when a page lands, and when a re-read lands rows that
  differ from the ones held or a refusal.

Native tests catch the one-way mistake: a change to the view's serialized
state without `cx.notify()` panics (debug builds). They cannot see a
`#[serde(skip)]` field or a child entity's state: notify for those yourself.
`TestAppContext::renders()` and `ticks()` count what the view did since it
opened, so a test can pin that an event draws nothing.

## The `View` trait

`View` (`src/root.rs`) is everything the host reads about a view, how it is
built and how it joins the host: `NAME` (the tab and catalog name), `DESCRIPTION` (one
catalog line, `""` by default), `ICON` (its icon where the app shows it
small, named as `svg().path(..)` names one the app bundles, the Lucide set
as `"icons/<lucide name>.svg"`; `""` by default, and then, or for a name the
app lacks, the app shows the name's initial), `TINT` (the colour of its tile
where the app lists it, a phone's Home: a `Tint` by name, `Tint::Blue`, whose
light and dark values are the app's; `Tint::None` by default, the neutral
tile), `CAPABILITIES` (the `methods::Capability`
halves of the kinds it asks through, `&[]` by default: a method whose
capability is not listed is refused `undeclared_capability`), `TARGETS` (the
programs it addresses with `op.submit`, `program.query` and `program.changes`,
by each `Program`'s `NAME`, `&[]` by default: a method naming another is
refused `undeclared_target`, and a view with `op` or `program` that names
none does not compile), `MIN_WINDOW_WIDTH` (the narrowest pointer window in px it works at,
default 480, at least 1 or it does not compile: on a desk the app never
lays it out narrower, and a narrower window scrolls it sideways; a touch
screen seats the view at its own width whatever this says, so it is not
lowered for phones), `new(window, cx)`, the state on
first mount (`Default::default()` unless the view says otherwise), and
`attach(window, cx)`, which runs on every mount, first or restored from a
snapshot, and starts what the view follows and reads. Subscribe in `attach`,
never in `new`: a restored view is not built again, and only what `attach`
starts follows the host after a redeploy. The snapshot is the view's own serde
as named MessagePack (`src/snapshot.rs`), refused while work is
pending. Derive `Default`/`Serialize`/`Deserialize`
and keep `Task`s out of the state (`Loadable` does). A program's type that
derives only borsh goes in the state as its bytes:
`#[serde(with = "view::borsh_bytes")]` on the field
(`src/borsh_bytes.rs`).

## Child entities

A view composes as a gpui view does, and one of the six does it.
`cx.new(|cx| Sidebar::new(cx))` builds a child entity from any `'static`
state, with no `View` of its own; a child that implements `Render` is a
child element (`.child(self.sidebar.clone())`) or a tooltip's content.
`entity.update(cx, |sidebar, cx| ..)` runs on it from a listener, another
entity or a task, and its `cx.notify()` renders the view. `cx.emit(event)`
tells what `cx.subscribe(&child, ..)`s, and `cx.observe(&child, ..)` hears
its notify, for as long as the `Subscription` is kept. The snapshot is the
root's serde alone: keep a child `#[serde(skip)]` in an `Option`, beside its
subscriptions, and build both in `attach`, so a restore builds them again.
A child is not a `View`: it logs with `cx.host().log(..)`.
`tests/gpui_entities.rs` is the whole of it.

## Exporting

`export_view!(View)` (`src/lib.rs`) writes the five wasm exports and the
manifest section `ducktape.view.manifest` from the trait's consts
(`view-wire/src/manifest.rs`: header, `NAME`, `DESCRIPTION`,
`CAPABILITIES`, `MIN_WINDOW_WIDTH` in decimal, `WIRE_ID`, `TARGETS`, `ICON`, `TINT`; the
line list is `manifest::LINES`, part of `schema.txt` and so of `WIRE_ID`).

The ABI gate (`view-wire/src/abi.rs`, `tools/check-view-abi.py`): exactly one
import, `ducktape_view.panicked`, and exactly five function exports,
`alloc`/`init`/`tick`/`snapshot`/`restore`. `make wasm-views` builds every
view in `VIEWS`, runs `wasm-opt`, checks the ABI and prints the size;
`make view-wasm-check`
proves nothing in `VIEW_LINKABLE` reaches `VIEW_FORBIDDEN`. Wire bytes are
pinned by `view-wire/tests/golden.rs`: a shape change fails it until the
fixtures are regenerated with `WIRE_GOLDEN_WRITE=1`, which moves `WIRE_ID`
(`view-wire/build.rs` hashes them); a host refuses a view built against
another.

## A notifier

What becomes a notice, for whom, its words, its tag, its link and when it
is retracted are decided by EACH PROGRAM in its own crate: the SDK fixes
the shape, and the app is a generic carrier. A program does it in its
NOTIFIER (`src/notifier.rs`), not in its pane's view: the app runs one per
program on the network in hand, whatever panes are open, so a notice is
posted once however many panes there are, and with none.

A notifier is a `Notifier` (`NAME`, `CAPABILITIES`, `TARGETS`, `new`)
built with the view's own `Context`: it follows, lands, asks the host and
keeps a store as a view does, and draws nothing. `export_notifier!(N)`
writes its three exports (`alloc`/`init`/`tick`: borsh answers in, a borsh
`Turn` out, `view-wire/src/notifier.rs`) and the same manifest section.
The view crate builds it alone under a `notifier` cargo feature, and a
founding packs it into the program's code blob as `ducktape.notify`. It
may not declare what needs a person in front of it (`op`, `invite`,
`link`, `clipboard`, `web`): the app refuses to start it.

Two host methods are its signals: `store.changes` (a key another instance
of the program, a pane, set in the store they share: a read cursor moved)
and `host.screen` (the routes the program's views show on screen, each
named by its view with `host.showing`: what the person is looking at gets
no notice). `testing::TestNotifier` drives one against a `FakeHost`.

## Testing

`testing::TestAppContext` (`src/testing/context.rs`) opens a view over a
`FakeHost` (`src/testing/fake_host.rs`) that answers by a request's shape.
An ask waits for its answer, so a test says what comes back:
`handle::<Method>`, `refuse`, or `never` for one that stays out; an ask
nothing answers fails the test, naming it. A subscription nothing feeds
stays open; `stream::<Method>()` is its feed, and a feed nobody hears
fails the test. `requests::<Method>()` is everything the view sent,
notifies too (`HostLog`, `LinkOpen`, `HostWidget`). Then
`simulate_click`, `simulate_input`, `simulate_event` and a typed
`simulate_*` per event; `texts`, `find`, `node(key).style()`/`.text()`/
`.children()`, `interactivity`, `last_frame`, and a `TickReport` per tick.
Keys go where the host sends them: `simulate_focus` (or a click, or the
view's own `Window::focus`) puts the keyboard on a node, and a key goes
down its focus path to the capture listeners and back up it; a key at a
node off that path fails the test. A click goes out from the node pressed
through every node around it with a click, as gpui passes it, until one
consumes it (`consumes_click`), hides what is behind it (`occlude`), or the
press leaves a dialog's layer. A field's text is the host's: it takes a
frame's text when the field is new or its generation moved, and tells the
view when what it took differs; `simulate_input` types into that text; a
field drawn from a `TextField` made in `render` is a new generation every
frame, and loses what is typed. The view opens in a pane the size the
app gives a new window (`testing::VIEWPORT`; `simulate_resize` moves it,
before or after the open), reads it as `Window::viewport_size()`, and a
list's first frame holds the rows that fill it; `simulate_viewport(rows)`
or `simulate_range(key, range)` then show it the rows the host shows.
A sensor hears nothing until `simulate_bounds(name, origin, size)` lays
its child out, and then as the app tells it: the same bounds again are no
event. A web piece hears its page through `simulate_web_state(name, state)`,
a camera, microphone or remote piece its device or its track through
`simulate_media_state(id, state)` (a `track` left `None` keeps the one a
`Relay` moves).
The reader's offset is UTC until `simulate_offset(minutes)`.
Every frame the view sends is held to `view_wire::audit`: a fault panics with
its kind and key path, so each screen a test reaches is gated.
Views that talk live are tested together over `testing::Relay`
(`src/testing/relay.rs`): `relay.link(&a).link(&b)`, one
`TestAppContext` each, and it answers their `live.*` requests as one node
would, the n-th view linked acting as account n. Its grants are the rows a
program writes, decided as the node decides them: every room starts with
`*` = `Grant::all()`, and `hold(room)`, `grant(room, who, grant)` and
`revoke(room, who)` are the block that takes every row away (each ask hears
`Waiting`), writes one or removes one, in the node's order: what the rows
no longer cover ends `not_granted`, publications first (their subscribers
hear `publisher_gone`), then subscriptions, then room lists; a room list
they still let watch hears the tracks still up that its row adds and drops;
then what waited and is covered now is answered. The views opened in one
`TestAppContext` are windows of one app: a second publication of a track it
publishes is refused `duplicate_track`, as the app refuses it. A call's
pieces ask as the app asks for them: a publishing camera or microphone is a
`live.publish` while its view's frames place it, a remote piece a
`live.subscribe`, and each hears its `MediaState::track` through `on_state`
when it changes; no object crosses. A piece its frame drops ends its track
`track_ended` for its subscribers.
`testing::lower(build)` is the one path that is not: it lowers a single
element with no host, sanitizer or audit, for a test of what that element
carries (`testing::assert_accessible(&tree)` audits it when the test wants).
It holds the view to its `View::CAPABILITIES` and `View::TARGETS` as the app
does: a method whose capability the manifest leaves out, or a web, camera,
microphone or remote piece from a view that does not declare `web`, `camera`,
`microphone` or (for a piece on a live track) `live`, panics with
`methods::refusal::UNDECLARED_CAPABILITY`, a node method naming a program
the targets leave out with `methods::refusal::UNDECLARED_TARGET`, the codes
the app refuses them with.
Screen export: a test gated on `*_SCREEN_EXPORT=1` (`FORGE_SCREEN_EXPORT`,
`crates/app/forge-view/src/screen_tests.rs`; `CHAT_SCREEN_EXPORT`,
`crates/app/chat-view/src/tests/mod.rs`) writes each screen's whole frame (its
tree and the styles it names, `TestAppContext::whole_frame`) as JSON under
`target/`; the app renders those with `dev/screens/chat-screens.sh
FIXTURES_DIR OUTPUT_DIR` (`ducktape-app --render-tree`, debug builds).
