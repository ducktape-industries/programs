#!/bin/sh
# `make new-program NAME=x` / `make new-view NAME=x-view`: a program in
# valset's shape (types and `describe` in lib.rs, the program in program.rs,
# its rules over `store` in rules.rs, a native test over `program::MockHost`
# in tests.rs; its wasm exports behind `program`, its describe module behind
# `describe`, its schema in crates/sdk/founding) or a view in members-view's
# shape (links its program with `program` off, `export_view!`, one screen
# test), registered in the Makefile
# and the workspace. `make scaffold-check` builds and tests both. Run from
# the repo root.
#   tools/scaffold.sh program <name> | view <name>-view
set -eu
kind=$1
name=$2
dir=crates/app/$name
case "$name" in
    *[!a-z0-9-]* | -* | *-) echo "$name: a crate name is lowercase words joined by '-'" >&2; exit 1 ;;
esac
test ! -e "$dir" || { echo "$dir exists" >&2; exit 1; }
snake=$(echo "$name" | tr - _)

# The lines registration edits, each of which must be there exactly once;
# every one is checked before anything is written, so a scaffold never
# half-registers.
MEMBERS='^    "crates/lib/gitcore",$'
DEPS='^forge = { path = "crates/app/forge" }$'
SCHEMAS=crates/sdk/founding/src/schema.rs
SCHEMA='^        _ => return None,$'
FOUNDING=crates/sdk/founding/Cargo.toml
FOUNDING_DEPS='^harness = { workspace = true }$'
anchor() { # <file> <pattern>
    test "$(grep -c "$2" "$1")" = 1 || { echo "$1: no single line matches '$2'; nothing was written. Fix the anchor in $0." >&2; exit 1; }
}
# <file> <sed script>: in place, the same with GNU and BSD sed (macOS has
# no bare `-i`, nor `\n` in a replacement: a new line there is `\` and a
# real line break).
edit() {
    sed -e "$2" "$1" > "$1.new" || { rm -f "$1.new"; exit 1; }
    mv "$1.new" "$1"
}
register() { # <Makefile list> <name>
    edit Makefile "s/^$1 := .*/& $2/"
    edit Cargo.toml "s|$MEMBERS|    \"crates/app/$2\",\\
&|"
}

program() {
    anchor Makefile '^PROGRAMS := '
    anchor Makefile '^VIEW_LINKABLE := '
    anchor Cargo.toml "$MEMBERS"
    anchor Cargo.toml "$DEPS"
    anchor "$SCHEMAS" "$SCHEMA"
    anchor "$FOUNDING" "$FOUNDING_DEPS"
    # The program type: TitleCase of the program name.
    title=$(echo "$name" | awk -F- '{ for (i = 1; i <= NF; i++) printf "%s%s", toupper(substr($i, 1, 1)), substr($i, 2) }')
    mkdir -p "$dir/src"
    cat > "$dir/Cargo.toml" <<EOF
[package]
name = "$name"
version.workspace = true
edition.workspace = true

# The types and rules are always built; a view links them with \`program\`
# off. \`program\` adds its wasm exports (\`program::export!\`), which only its
# own wasm build turns on.
[lib]
crate-type = ["cdylib", "rlib"]

[features]
program = []
# \`describe\` makes this crate's wasm build the \`ducktape.describe\` module:
# the \`describe\` export alone, no program, no imports.
describe = []

[dependencies]
abi = { workspace = true }
borsh = { workspace = true }
describe = { workspace = true }
program = { workspace = true }
store = { workspace = true }
EOF
    cat > "$dir/src/lib.rs" <<EOF
//! The \`$name\` program: one count, to be replaced by what it keeps. The
//! types, rules and [\`$title\`] program are always built; a view links them
//! with \`program\` off. The layout, in reading order:
//!
//! - \`lib.rs\` (here): the types on the wire, all borsh, and [\`describe()\`]
//! - \`program.rs\`: [\`$title\`], the program: its name and types, which
//!   a view names it by, then one match over every op and one over every
//!   query
//! - \`rules.rs\`: what each op checks and writes, over \`store\`
//! - \`tests.rs\`: the program natively over \`program::MockHost\`
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use program::$title;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "$name";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Bump { by: u64 },
}

/// A read. Each variant names the reply that answers it, so a view asks
/// it alone (\`host.query($snake::ask::Count)\`) and gets that reply.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask)]
#[ask($title)]
pub enum Query {
    #[ask(Reply::Count(u64))]
    Count,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Count(u64),
}

/// An op as a person reads it: a title and its fields. The source of the
/// \`ducktape.describe\` module this program ships (\`make wasm-describes\`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Bump { by } => (
            format!("Bump by {by}"),
            vec![field("by", Value::count(*by))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (\`describe\`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Bump"]);
}
EOF
    cat > "$dir/src/program.rs" <<EOF
//! The program: every op and every query, each handed to its rule.

use ::program::{Error, ExecCtx, Program, QueryCtx};

use crate::rules::{bump, count};
use crate::{PROGRAM, Op, Query, Reply};

pub struct $title;

impl Program for $title {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        // A write acts as an account: a key that holds none is refused.
        ctx.sender()?;
        match op {
            Op::Bump { by } => bump(ctx, by),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Count => Reply::Count(count(ctx)?),
        })
    }
}

#[cfg(feature = "program")]
::program::export!($title);
EOF
    cat > "$dir/src/rules.rs" <<EOF
// The rules: what each op checks, then what it writes. A rule checks before
// it writes, so a refusal needs no rollback.

use ::program::{Error, ExecCtx, QueryCtx};
use store::Item;

/// The one value this program keeps.
const COUNT: Item<u64> = Item::new("count");

pub(crate) fn bump(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
    COUNT.update(ctx, |count| *count = count.saturating_add(by))?;
    Ok(())
}

pub(crate) fn count(ctx: &QueryCtx) -> Result<u64, Error> {
    Ok(COUNT.get(ctx)?.unwrap_or_default())
}
EOF
    cat > "$dir/src/tests.rs" <<EOF
// The program natively over \`program::MockHost\`, as the host runs it.

use ::program::{Env, MockHost, Program, code};

use crate::{$title, PROGRAM, Op, Query, Reply};

/// \`MockHost::env\` is a person's frame: signed by key \`[1; 32]\`, which
/// holds account 1. The chain's own is \`.root()\`.
#[test]
fn bumps_add_up_and_read_back() {
    let host = MockHost::default();
    let first = host.exec(MockHost::env(PROGRAM));
    $title::execute(&first, Op::Bump { by: 2 }).unwrap();
    let later = Env {
        height: 2,
        ..MockHost::env(PROGRAM)
    };
    $title::execute(&host.exec(later), Op::Bump { by: 3 }).unwrap();
    let count = $title::query(&host.query(MockHost::env(PROGRAM)), Query::Count).unwrap();
    assert_eq!(count, Reply::Count(5));
}

#[test]
fn a_key_that_holds_no_account_writes_nothing() {
    let host = MockHost::default();
    let unheld = MockHost::env(PROGRAM).signed([2u8; 32], None);
    let refusal = host.refused(|| $title::execute(&host.exec(unheld), Op::Bump { by: 1 }));
    assert_eq!(refusal.code, code::UNAUTHORIZED);
}
EOF
    register PROGRAMS "$name"
    edit Makefile "s/^VIEW_LINKABLE := .*/& $name/"
    edit Cargo.toml "s|$DEPS|&\\
$name = { path = \"$dir\" }|"
    edit "$SCHEMAS" "s|$SCHEMA|        \"$name\" => interface::<$snake::$title>(),\\
&|"
    edit "$FOUNDING" "s|$FOUNDING_DEPS|&\\
$name = { workspace = true }|"
    cat <<EOF
$dir/{Cargo.toml,src/{lib,program,rules,tests}.rs}, PROGRAMS, VIEW_LINKABLE, workspace members and dependencies, its schema in $SCHEMAS.
Next:
  1. write the contract (Op/Query/Reply and describe() in lib.rs, the program in program.rs, its rules in rules.rs); \`make dev P=$name\` builds and tests it
  2. \`make new-view NAME=$name-view\` for its screen
  3. found it: a [[programs]] entry (id "$name", code "$snake.wasm") in crates/sdk/founding/founding.toml, and in qa's founding.toml
     (code "@PROGRAMS@/$snake.wasm"); its view goes in crates/sdk/founding's PROGRAMS (step 2 prints it)
EOF
}

view() {
    case "$name" in *-view) ;; *) echo "$name: a view is named <program>-view" >&2; exit 1 ;; esac
    program=${name%-view}
    program_snake=$(echo "$program" | tr - _)
    test -d "crates/app/$program" || { echo "crates/app/$program is not there: make new-program NAME=$program first" >&2; exit 1; }
    anchor Makefile '^VIEWS := '
    anchor Cargo.toml "$MEMBERS"
    upper=$(echo "$program_snake" | tr a-z A-Z)
    # The view type: TitleCase of the program name.
    title=$(echo "$program" | awk -F- '{ for (i = 1; i <= NF; i++) printf "%s%s", toupper(substr($i, 1, 1)), substr($i, 2) }')
    mkdir -p "$dir/src"
    cat > "$dir/Cargo.toml" <<EOF
[package]
name = "$name"
edition.workspace = true
version.workspace = true
publish = false

# A Rust-authored view, exported as a dynamically loaded wasm module by
# \`export_view!\`; \`unsafe_code\` is not forbidden only because the module
# exports need \`#[unsafe(export_name)]\`.
[lib]
crate-type = ["cdylib"]

[dependencies]
view.workspace = true
# The program with \`program\` off: its types and the program this view
# names it by, no host import.
$program.workspace = true
serde.workspace = true

[dev-dependencies]
serde_json.workspace = true
EOF
    cat > "$dir/src/lib.rs" <<EOF
//! $title: the count the \`$program\` program keeps, re-read on every live
//! bump of the program.
// every SDK name this file writes: gpui's (elements, styles, \`Render\`,
// \`Context\`, \`Window\`), the view's (\`View\`, \`Loadable\`, \`Host\`,
// \`Error\`, \`export_view!\`) and the methods' (\`Capability\`, \`Changes\`)
use view::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct $title {
    count: Loadable<u64>,
    /// A child entity. The snapshot carries this root alone, so \`attach\`
    /// builds its children on every mount.
    #[serde(skip)]
    heading: Option<Entity<Heading>>,
}

/// The heading, as a child entity: state and a render of its own, built
/// with \`cx.new\` and placed in the tree by its handle; no \`View\`, no
/// snapshot.
struct Heading {
    text: SharedString,
}

impl View for $title {
    const NAME: &'static str = "$title";
    const DESCRIPTION: &'static str = "The count the $program program keeps.";
    const CAPABILITIES: &'static [Capability] = &[Capability::Program];
    const TARGETS: &'static [&'static str] = &[$program_snake::PROGRAM];
    const MIN_WINDOW_WIDTH: u32 = 480;

    /// Follows the program and reads the count: on the first mount and
    /// after every redeploy, which restores the view from its snapshot.
    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<Changes<$program_snake::$title>>((), |view, _, cx| view.read(cx))
            .detach();
        self.read(cx);
        self.heading = Some(cx.new(|_| Heading {
            text: "$title".into(),
        }));
    }
}

impl Render for Heading {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("$program-title")
            .text_size(px(16.))
            .font_weight(view::FontWeight::SEMIBOLD)
            .role(view::Role::Heading)
            .aria_level(1)
            .child(self.text.clone())
    }
}

impl Render for $title {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let body = match &self.count {
            Loadable::Idle | Loadable::Loading(_) => "Reading…".to_owned(),
            Loadable::Ready(count) | Loadable::Reloading(count, _) => format!("Count: {count}"),
            Loadable::Failed(refusal) => refusal.message.clone(),
        };
        div()
            .id("$program")
            .flex()
            .flex_col()
            .gap_3()
            .p_5()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_size(px(13.))
            .children(self.heading.clone())
            .child(div().id("$program-body").child(body))
    }
}

impl $title {
    /// One read: the boot, a restore, a live bump. A value already on
    /// screen stays there while it runs, and a bump that lands the same
    /// count draws nothing.
    fn read(&mut self, cx: &mut Context<Self>) {
        cx.load(self, count(cx.host()), |view| &mut view.count);
    }
}

async fn count(host: Host) -> Result<u64, Error> {
    host.query($program_snake::ask::Count).await
}

export_view!($title);

#[cfg(test)]
mod tests;
EOF
    cat > "$dir/src/tests.rs" <<EOF
use super::*;
use view::methods::{Change, Query};
use view::testing::TestAppContext;

fn ready() -> TestAppContext {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<Query<$program_snake::$title>>(|_| Ok($program_snake::Reply::Count(5)));
    cx.open::<$title>();
    cx.run_until_parked();
    cx
}

/// A live bump re-reads the count; a re-read the program refuses says why,
/// where the count was.
#[test]
fn a_refused_re_read_says_why() {
    let mut cx = ready();
    let bumps = cx.host().stream::<Changes<$program_snake::$title>>();
    cx.host()
        .refuse::<Query<$program_snake::$title>>("unavailable", "The count cannot be read.");
    bumps.send(Some(Change {
        height: 2,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(cx.has_text("The count cannot be read."), "{:?}", cx.texts());
}

/// The ready screen; \`${upper}_SCREEN_EXPORT=1\` also writes its tree for the
/// app's node-less renderer (\`ducktape-app --render-tree <json>\`).
#[test]
fn the_ready_screen_shows_the_count() {
    let cx = ready();
    assert!(cx.has_text("Count: 5"), "{:?}", cx.texts());
    if std::env::var_os("${upper}_SCREEN_EXPORT").is_none() {
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/$program-screens");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(
        out.join("01-ready-light.json"),
        serde_json::to_vec(cx.root()).unwrap(),
    )
    .unwrap();
}
EOF
    register VIEWS "$name"
    grep -q 'Count' "crates/app/$program/src/lib.rs" || echo "note: $program has no Query::Count; the screen's count() in $dir/src/lib.rs asks it, so step 1 comes before step 2"
    cat <<EOF
$dir/{Cargo.toml,src/lib.rs,src/tests.rs}, VIEWS and workspace members.
Next:
  1. replace the screen in src/lib.rs with what the program's Query answers (it asks \`Query::Count\` until then)
  2. \`make dev P=$program V=$name\` tests both natively, then builds them and gates the view (ABI), which needs wasm-tools and python3 on PATH
  3. pack it into its program: ("$program_snake", "$snake") in PROGRAMS (crates/sdk/founding/src/lib.rs); then in qa \`kit up NAME\`
EOF
}

# Each on its own line: `set -e` does not reach into a function run on the
# left of `&&`.
case "$kind" in
    program) program ;;
    view) view ;;
    *) echo "usage: tools/scaffold.sh program <name> | view <name>-view" >&2; exit 1 ;;
esac
# Import order and line width follow the name, so rustfmt has the last word.
${CARGO:-cargo} fmt -p "$name"
