//! The wire's shape, committed as `tests/golden/schema.txt` beside the bytes
//! and hashed into `WIRE_ID` with them. The bytes hold what the fixtures
//! sample; the shape holds what the types declare, sampled or not: every
//! field, one `skip_serializing_if` leaves out of the bytes too, every
//! variant of every enum the wire reaches, and every method kind with its
//! target, request and reply. A shape change fails here until regenerated,
//! and regenerating moves `WIRE_ID`.
//!
//! The tree half is serde's, traced from the `Deserialize` side as the host
//! reads it: `Frame`, `Event`, `WidgetCommand` and the `Call` envelope, and
//! every container they reach. No name of it crosses: a struct is its
//! fields in the order listed here, a variant its index, and a sparse
//! struct (`Interactivity`, `Aria`) the fields it sets, each under its
//! index. So the shape is all that says which field a byte is, and a field
//! added or moved moves `WIRE_ID` through this file.
//!
//! A style crosses as a table entry, bytes to the tree (`Frame.styles`).
//! The style half is what an entry holds: gpui's `StyleRefinement` and
//! every container under it, traced the same way. An entry's bitmaps count
//! each refinement's fields in the order listed here (a test below holds
//! the entry codec to it), and an enum under it crosses as its index here.
//!
//! The method half is borsh's own schema of each method's request and
//! reply. What no half holds: the encode functions (`Call` wrapping a node
//! method's body, `op.submit`'s raw reply, `program.changes` sending the
//! program's name) are `methods.bin`'s alone, an entry's own value grammar
//! (a length, a colour: `styles/entry.rs`) is `frame.bin`'s, and a type the
//! tree writes as a string (a colour in a text run) is `STR` whatever its
//! grammar.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;

use borsh::schema::{Declaration, Definition};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use gpui::{FontFeatures, StyleRefinement};
use serde::de::DeserializeOwned;
use serde_reflection::{
    ContainerFormat, Format, FormatHolder, Named, Registry, Samples, Tracer, TracerConfig,
    VariantFormat,
};
use view_wire::methods::{self, Method, Program};
use view_wire::{Event, Frame, Node, Patch, WidgetCommand};

use super::golden;

/// A tracer whose stand-in string is a colour: gpui writes `Hsla` as hex,
/// and a colour is the one checked string it decodes outside an `Option`,
/// where a refusal would not be cut on the next pass (see [`trace`]).
fn tracer() -> Tracer {
    Tracer::new(
        TracerConfig::default()
            .default_string_value("#000".into())
            .default_borrowed_str_value("#000"),
    )
}

/// Traces `T` from the `Deserialize` side until every format it reaches is
/// known, and answers its own. A decoder that checks what it decodes
/// refuses the tracer's stand-in (an empty name, a zero-length chunk), but
/// what the pass reached before the refusal is kept, and a sequence or
/// option already known is empty on the next pass, so each pass gets
/// further than the last.
fn trace<T: DeserializeOwned>(tracer: &mut Tracer) -> Format {
    let samples = Samples::new();
    let mut refusal = None;
    for _ in 0..1000 {
        match tracer.trace_type::<T>(&samples) {
            Ok((format, _)) => return format,
            Err(error) => refusal = Some(error),
        }
    }
    panic!("{} does not trace: {refusal:?}", std::any::type_name::<T>())
}

type Trace = fn(&mut Tracer) -> Format;

/// Every container `traces` reach, by name, each one whole: every format
/// known, and every enum at its declared variants, which a trace reaches
/// only for an enum traced on its own (a pass revisits a nested enum at its
/// first variant). One partial fails here by name.
fn registry(traces: &[Trace]) -> Registry {
    let mut tracer = tracer();
    for trace in traces {
        trace(&mut tracer);
    }
    let mut registry = tracer.registry_unchecked();
    font_features(&mut registry);
    action_data(&mut registry);
    let partial: Vec<&String> = registry
        .iter_mut()
        .filter_map(|(name, container)| {
            let whole = container.normalize().is_ok()
                && match container {
                    ContainerFormat::Enum(variants) => {
                        variants.keys().copied().eq(0..variants.len() as u32)
                    }
                    _ => true,
                };
            (!whole).then_some(name)
        })
        .collect();
    assert!(
        partial.is_empty(),
        "the wire does not trace whole: trace each of {partial:?} on its own"
    );
    registry
}

/// gpui's `FontFeatures` reads each tag's value through an untagged enum,
/// which no trace can enter (`deserialize_any`), so the trace leaves it
/// unknown: it is what `FontFeatures` writes instead, traced from the
/// `Serialize` side.
fn font_features(registry: &mut Registry) {
    let features = FontFeatures(Arc::new(vec![("calt".into(), 1)]));
    let (written, _) = tracer()
        .trace_value(&mut Samples::new(), &features)
        .unwrap();
    let optional = Format::Option(Box::new(written.clone()));
    for (container, format) in [("TextRun", written), ("TextStyleRefinement", optional)] {
        if let Some(ContainerFormat::Struct(fields)) = registry.get_mut(container) {
            let field = fields
                .iter_mut()
                .find(|field| field.name == "font_features")
                .expect("font_features");
            // Only the map's values are past the trace: a field retyped to
            // anything but a map is a shape of its own, not this one.
            let traced = format!("{:?}", field.value);
            assert!(
                traced.contains("Map {"),
                "{container}.font_features is {traced}"
            );
            field.value = format;
        }
    }
}

/// accesskit's `ActionData` (an `A11yAction`'s data), whole, in place of the
/// tree's partial one. Its `Point` is accesskit's f64 one, which a trace
/// beside gpui's `Point<Pixels>` would refuse by name: so it is traced alone
/// and [`merge`]d as `accesskit`.
fn action_data(registry: &mut Registry) {
    if registry.remove("ActionData").is_none() {
        return;
    }
    let mut tracer = tracer();
    trace::<accesskit::ScrollUnit>(&mut tracer);
    trace::<accesskit::ScrollHint>(&mut tracer);
    trace::<accesskit::ActionData>(&mut tracer);
    let own = tracer.registry().expect("ActionData traces whole alone");
    merge(registry, own, "accesskit");
}

/// `own`'s containers into `registry`: one `registry` already names
/// otherwise enters as `<prefix>::<name>`, and so does every reference to it.
fn merge(registry: &mut Registry, own: Registry, prefix: &str) {
    let clash: BTreeSet<String> = own
        .iter()
        .filter(|&(name, container)| registry.get(name).is_some_and(|tree| tree != container))
        .map(|(name, _)| name.clone())
        .collect();
    for (name, mut container) in own {
        container
            .visit_mut(&mut |format| {
                if let Format::TypeName(name) = format
                    && clash.contains(name)
                {
                    *name = format!("{prefix}::{name}");
                }
                Ok(())
            })
            .unwrap();
        let name = match clash.contains(&name) {
            true => format!("{prefix}::{name}"),
            false => name,
        };
        registry.insert(name, container);
    }
}

/// gpui's `Background`, written once per tag: its tag is crate-private to
/// gpui, so no trace can name it on its own, and a variant written before
/// the trace reaches it is one the trace takes as whole.
fn backgrounds(tracer: &mut Tracer) -> Format {
    let color = gpui::black();
    let stop = gpui::linear_color_stop(color, 0.);
    let mut samples = Samples::new();
    for background in [
        gpui::solid_background(color),
        gpui::linear_gradient(0., stop, stop),
        gpui::pattern_slash(color, 1., 1.),
        gpui::checkerboard(color, 1.),
    ] {
        tracer.trace_value(&mut samples, &background).unwrap();
    }
    trace::<gpui::Background>(tracer)
}

/// A `bin` as the tracer reads one. A reply's `Result` holds its bytes as
/// a `bin` (`codec/bin.rs`), and a `Result` is whole only traced on its own:
/// this stands where the crate's own reader does, and the trace refuses it
/// if `Event::Response` reads anything else there.
struct Bin;

impl<'de> serde::Deserialize<'de> for Bin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bytes;
        impl serde::de::Visitor<'_> for Bytes {
            type Value = Bin;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("bytes")
            }
            fn visit_bytes<E: serde::de::Error>(self, _: &[u8]) -> Result<Bin, E> {
                Ok(Bin)
            }
        }
        deserializer.deserialize_bytes(Bytes)
    }
}

/// What the tree's trace starts from: what crosses, and every enum of
/// view-wire's, accesskit's and gpui's under it.
const TREE: &[Trace] = &[
    trace::<Frame>,
    trace::<Event>,
    trace::<WidgetCommand>,
    trace::<methods::Call>,
    trace::<Node>,
    trace::<Patch>,
    trace::<Result<Bin, view_wire::Error>>,
    trace::<view_wire::ElementIdWire>,
    trace::<view_wire::Anchor>,
    trace::<view_wire::AnchoredFitMode>,
    trace::<view_wire::AnchoredPositionMode>,
    trace::<view_wire::Autocapitalize>,
    trace::<view_wire::VirtualKeyboard>,
    trace::<view_wire::TextInputAction>,
    trace::<view_wire::TextTransform>,
    trace::<view_wire::CanvasCommand>,
    trace::<view_wire::CanvasLineCap>,
    trace::<view_wire::CanvasLineJoin>,
    trace::<view_wire::CanvasSegment>,
    trace::<view_wire::CanvasShape>,
    trace::<view_wire::DispatchPhase>,
    trace::<view_wire::ElementIdAtom>,
    trace::<view_wire::HoverListenerMode>,
    trace::<view_wire::ImageData>,
    trace::<view_wire::ImageObjectFit>,
    trace::<view_wire::ListAlignment>,
    trace::<view_wire::ListCommand>,
    trace::<view_wire::ListSizingBehavior>,
    trace::<view_wire::RichTextRuns>,
    trace::<view_wire::SvgSource>,
    trace::<view_wire::WebCommand>,
    trace::<view_wire::MicCommand>,
    trace::<view_wire::MediaStatus>,
    trace::<view_wire::Playback>,
    trace::<view_wire::RemoteMedia>,
    trace::<view_wire::TrackState>,
    trace::<view_wire::click::Click>,
    trace::<view_wire::click::KeyboardButton>,
    trace::<view_wire::click::MouseButton>,
    trace::<view_wire::interactivity::PressureStage>,
    trace::<view_wire::interactivity::TouchPhase>,
    trace::<view_wire::keyboard::Key>,
    trace::<view_wire::keyboard::Location>,
    trace::<view_wire::keyboard::Named>,
    trace::<view_wire::keyboard::NativeCode>,
    trace::<view_wire::keyboard::Physical>,
    trace::<view_wire::list::UniformListHorizontalSizing>,
    trace::<view_wire::list::UniformListScrollStrategy>,
    trace::<view_wire::list::UniformListSizing>,
    trace::<view_wire::mouse::Cursor>,
    trace::<view_wire::mouse::ScrollDelta>,
    trace::<accesskit::Action>,
    trace::<accesskit::AriaCurrent>,
    trace::<accesskit::HasPopup>,
    trace::<accesskit::Invalid>,
    trace::<accesskit::Live>,
    trace::<accesskit::Orientation>,
    trace::<accesskit::Role>,
    trace::<accesskit::Toggled>,
    trace::<gpui::FontStyle>,
];

/// What the style's trace starts from: gpui's backgrounds, its enums under
/// a style, and the style.
const STYLE: &[Trace] = &[
    backgrounds,
    trace::<gpui::AlignContent>,
    trace::<gpui::AlignItems>,
    trace::<gpui::BorderStyle>,
    trace::<gpui::ColorSpace>,
    trace::<gpui::CursorStyle>,
    trace::<gpui::Display>,
    trace::<gpui::Fill>,
    trace::<gpui::FlexDirection>,
    trace::<gpui::FlexWrap>,
    trace::<gpui::FontStyle>,
    trace::<gpui::GridPlacement>,
    trace::<gpui::GridTemplateMinSize>,
    trace::<gpui::Overflow>,
    trace::<gpui::Position>,
    trace::<gpui::TextAlign>,
    trace::<gpui::TextOverflow>,
    trace::<gpui::Visibility>,
    trace::<gpui::WhiteSpace>,
    trace::<StyleRefinement>,
];

/// Every container the tree reaches, and every one a style entry holds
/// that the tree does not hold too. They are traced apart: a trace names a
/// container by its serde name alone, and the tree's text ranges are
/// `Range<usize>` where a style's grid placement is a
/// `Range<GridPlacement>`.
pub(super) fn tree() -> (Registry, Registry) {
    let tree = registry(TREE);
    let mut style = registry(STYLE);
    // gpui's debug outlines are fields of its debug build alone, and no
    // entry carries them: the shape is the same in either build
    let Some(ContainerFormat::Struct(fields)) = style.get_mut("StyleRefinement") else {
        unreachable!("StyleRefinement is a struct")
    };
    fields.retain(|field| !["debug", "debug_below"].contains(&field.name.as_str()));
    style.retain(|name, container| tree.get(name) != Some(container));
    (tree, style)
}

/// The backgrounds an entry does not carry: gpui gives a pattern's payload
/// no public read, so one crosses as its kind alone and the host refuses
/// the entry (`style_sanitize`'s own rule, held by its test).
const PATTERNS: [&str; 2] = ["PatternSlash", "Checkerboard"];

/// What the samples must show: [`tree`]'s two halves as one registry, the
/// style's `Range<GridPlacement>` as `style::Range`. A table entry stands
/// as the style it holds, which is what its JSON twin shows, less the
/// [`PATTERNS`] no entry carries.
pub(super) fn wire() -> Registry {
    let (mut registry, style) = tree();
    merge(&mut registry, style, "style");
    let Some(ContainerFormat::Struct(frame)) = registry.get_mut("Frame") else {
        unreachable!("Frame is a struct")
    };
    let styles = frame
        .iter_mut()
        .find(|field| field.name == "styles")
        .expect("Frame.styles");
    assert_eq!(styles.value, Format::Seq(Box::new(Format::Bytes)));
    styles.value = Format::Seq(Box::new(Format::TypeName("StyleRefinement".into())));
    let Some(ContainerFormat::Enum(tags)) = registry.get_mut("BackgroundTag") else {
        unreachable!("BackgroundTag is an enum")
    };
    let before = tags.len();
    tags.retain(|_, tag| !PATTERNS.contains(&tag.name.as_str()));
    assert_eq!(tags.len() + PATTERNS.len(), before);
    registry
}

/// The entry codec's bitmaps count each refinement's fields in the order
/// `schema.txt` lists them, so the shape names every bit.
#[test]
fn a_style_entry_counts_its_fields_as_the_shape_lists_them() {
    let (_, style) = tree();
    for (name, fields) in view_wire::entry_fields() {
        let ContainerFormat::Struct(listed) = &style[name] else {
            panic!("{name} is a struct")
        };
        let listed: Vec<&str> = listed.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(listed, fields, "{name}");
    }
}

/// The program a node method addresses, as [`Program`] declares it: each of
/// its types is a marker, so the schema reads `ProgramQuery` where a view's
/// program puts its own.
struct P;
impl Program for P {
    const NAME: &'static str = "P";
    type Op = ProgramOp;
    type Query = ProgramQuery;
    type Reply = ProgramReply;

    fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
        unreachable!()
    }

    fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
        unreachable!()
    }
}
#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
struct ProgramOp;
#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
struct ProgramQuery;
#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
struct ProgramReply;

pub(super) type Definitions = BTreeMap<Declaration, Definition>;

/// A method's line: its target, its request and reply, and whether borsh
/// carries them (else the tree's codec, serde's trace).
pub(super) struct Shape {
    pub(super) target: Option<&'static str>,
    pub(super) request: String,
    pub(super) reply: String,
    pub(super) borsh: bool,
}

/// A borsh method, its request and reply declared and defined.
fn borsh<M: Method>(definitions: &mut Definitions) -> (&'static str, Shape)
where
    M::Request: BorshSchema,
    M::Reply: BorshSchema,
{
    M::Request::add_definitions_recursively(definitions);
    M::Reply::add_definitions_recursively(definitions);
    let shape = Shape {
        target: M::TARGET,
        request: M::Request::declaration(),
        reply: M::Reply::declaration(),
        borsh: true,
    };
    (M::KIND, shape)
}

/// A method on the tree's side of the codec rule, its request and reply as
/// serde traces them.
fn tree_method<M: Method>() -> (&'static str, Shape)
where
    M::Request: DeserializeOwned,
    M::Reply: DeserializeOwned,
{
    let mut tracer = tracer();
    let shape = Shape {
        target: M::TARGET,
        request: format!("{:?}", trace::<M::Request>(&mut tracer)),
        reply: format!("{:?}", trace::<M::Reply>(&mut tracer)),
        borsh: false,
    };
    (M::KIND, shape)
}

/// Every method in `ALL` by kind, and the borsh definitions they reach.
pub(super) fn shapes() -> (BTreeMap<&'static str, Shape>, Definitions) {
    use methods::*;
    let mut definitions = Definitions::new();
    let d = &mut definitions;
    let shapes: BTreeMap<&str, Shape> = [
        borsh::<Query<P>>(d),
        borsh::<Submit<P>>(d),
        borsh::<Changes<P>>(d),
        tree_method::<HostWidget>(),
        borsh::<ChainStatus>(d),
        borsh::<InviteCreate>(d),
        borsh::<ChainBlocks>(d),
        borsh::<ChainBlock>(d),
        borsh::<ChainTx>(d),
        borsh::<ChainNetwork>(d),
        borsh::<BlobGet>(d),
        borsh::<BlobManifest>(d),
        borsh::<HostSession>(d),
        borsh::<HostVisible>(d),
        borsh::<HostFloat>(d),
        borsh::<HostUnfloat>(d),
        borsh::<HostPlacement>(d),
        borsh::<HostBadge>(d),
        borsh::<HostTitle>(d),
        borsh::<HostShow>(d),
        borsh::<CameraDevices>(d),
        borsh::<MicrophoneDevices>(d),
        borsh::<MicrophoneChime>(d),
        borsh::<FilePick>(d),
        borsh::<OpPut>(d),
        borsh::<FileRelease>(d),
        borsh::<FileSave>(d),
        borsh::<ClipboardFiles>(d),
        borsh::<LinkOpen>(d),
        borsh::<LinkWindow>(d),
        borsh::<HostRoute>(d),
        borsh::<HostId>(d),
        borsh::<ClockTicks>(d),
        borsh::<HostLog>(d),
        borsh::<ClipboardRead>(d),
        borsh::<ClipboardWrite>(d),
        borsh::<NotifyPost>(d),
        borsh::<NotifySeen>(d),
        borsh::<StoreGet>(d),
        borsh::<StoreSet>(d),
        borsh::<StoreChanges>(d),
        borsh::<StoreList>(d),
        borsh::<HostShowing>(d),
        borsh::<HostScreen>(d),
        borsh::<ChainHeads>(d),
        borsh::<ProgramDescribe>(d),
        borsh::<LivePublishTrack>(d),
        borsh::<LiveSendObject>(d),
        borsh::<LiveSubscribeTrack>(d),
        borsh::<LiveTracks>(d),
    ]
    .into();
    let kinds: BTreeSet<&str> = shapes.keys().copied().collect();
    let all: BTreeSet<&str> = ALL.iter().copied().collect();
    assert_eq!(kinds, all, "one line per method in ALL");
    (shapes, definitions)
}

/// The whole text: the methods, the borsh definitions they reach, and the
/// tree's containers.
fn schema() -> String {
    let (shapes, definitions) = shapes();
    let (tree, style) = tree();
    let mut text = String::from("# methods: kind, target, request -> reply; borsh unless named\n");
    for (kind, shape) in &shapes {
        let codec = if shape.borsh { "" } else { " tree" };
        writeln!(
            text,
            "{kind} {:?} {} -> {}{codec}",
            shape.target, shape.request, shape.reply
        )
        .unwrap();
    }
    text.push_str("\n# borsh: every definition the methods reach\n");
    for (declaration, definition) in &definitions {
        writeln!(text, "{declaration} = {definition:?}").unwrap();
    }
    text.push_str(
        "\n# tree: MessagePack by position (a struct its fields in this order, \
         a variant its index), every container by name\n",
    );
    render(&mut text, &tree);
    text.push_str(
        "\n# style: what a table entry holds, a bitmap of each refinement's fields \
         in this order and then the ones set\n",
    );
    render(&mut text, &style);
    text.push_str("\n# notifier: what its tick takes -> what it answers, borsh\n");
    let mut notifier = Definitions::new();
    <Vec<view_wire::notifier::Answer>>::add_definitions_recursively(&mut notifier);
    view_wire::notifier::Turn::add_definitions_recursively(&mut notifier);
    writeln!(
        text,
        "tick {} -> {}",
        <Vec<view_wire::notifier::Answer>>::declaration(),
        view_wire::notifier::Turn::declaration()
    )
    .unwrap();
    for (declaration, definition) in &notifier {
        writeln!(text, "{declaration} = {definition:?}").unwrap();
    }
    text.push_str("\n# manifest: one line each, in order\n");
    for line in view_wire::manifest::LINES {
        writeln!(text, "{line}").unwrap();
    }
    text
}

/// A registry as text: a line per container, and under it one per field or
/// variant (and a variant's fields), in the order they are written.
fn render(text: &mut String, registry: &Registry) {
    let fields = |text: &mut String, fields: &[Named<Format>], indent: &str| {
        for field in fields {
            writeln!(text, "{indent}{}: {:?}", field.name, field.value).unwrap();
        }
    };
    for (name, container) in registry {
        match container {
            ContainerFormat::Struct(named) => {
                writeln!(text, "{name} struct").unwrap();
                fields(text, named, "  ");
            }
            ContainerFormat::Enum(variants) => {
                writeln!(text, "{name} enum").unwrap();
                for (index, variant) in variants {
                    match &variant.value {
                        VariantFormat::Struct(named) => {
                            writeln!(text, "  {index} {} struct", variant.name).unwrap();
                            fields(text, named, "    ");
                        }
                        value => writeln!(text, "  {index} {}: {value:?}", variant.name).unwrap(),
                    }
                }
            }
            other => writeln!(text, "{name} {other:?}").unwrap(),
        }
    }
}

/// Bytes cross as a `bin`, one copy each way (`codec/bin.rs`). A byte field
/// that does not say so would cross as an array of integers and be read an
/// element at a time: the shape names it here, so it cannot be added
/// without failing.
#[test]
fn no_bytes_cross_as_an_array_of_integers() {
    let (tree, style) = tree();
    let mut arrays = Vec::new();
    for (name, container) in tree.iter().chain(&style) {
        container
            .visit(&mut |format| {
                if let Format::Seq(content) | Format::TupleArray { content, .. } = format
                    && **content == Format::U8
                {
                    arrays.push(name.as_str());
                }
                Ok(())
            })
            .unwrap();
    }
    assert!(
        arrays.is_empty(),
        "bytes that cross as an array of integers, in {arrays:?}: \
         mark the field `#[serde(with = \"crate::codec::bin\")]`"
    );
}

const MESSAGE: &str = "the wire's shape changed: if intended, regenerate with \
     WIRE_GOLDEN_WRITE=1 (this changes WIRE_ID)";

#[test]
fn the_wire_has_the_committed_shape() {
    let path = golden("schema.txt");
    let built = schema();
    if std::env::var_os("WIRE_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, &built).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}; {MESSAGE}", path.display()));
    if let Some((line, (was, is))) = committed
        .lines()
        .zip(built.lines())
        .enumerate()
        .find(|(_, (was, is))| was != is)
    {
        panic!(
            "{MESSAGE} (schema.txt line {}: `{was}` is now `{is}`)",
            line + 1
        );
    }
    assert!(committed == built, "{MESSAGE} (schema.txt grew or shrank)");
}

/// The files `build.rs` hashes into `WIRE_ID`, in its order.
const HASHED: [&str; 3] = ["frame.bin", "methods.bin", "schema.txt"];

/// The gap the schema closes, held generally: every name the wire has — a
/// container, a field, a variant, a method kind — is in what `WIRE_ID`
/// hashes, so none comes or goes without moving it. The bytes alone fail
/// this even with every variant and field sampled (`coverage.rs`): neither
/// borsh nor the tree's codec writes a name at all.
#[test]
fn wire_id_hashes_every_name_on_the_wire() {
    let hashed: Vec<u8> = HASHED
        .iter()
        .flat_map(|name| std::fs::read(golden(name)).unwrap())
        .collect();
    let id = hashed.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    assert_eq!(
        format!("{id:016x}"),
        view_wire::WIRE_ID,
        "build.rs hashes {HASHED:?}"
    );

    let hashed = String::from_utf8_lossy(&hashed);
    let (tree, style) = tree();
    let mut names: BTreeSet<&str> = methods::ALL.iter().copied().collect();
    for (name, container) in tree.iter().chain(&style) {
        names.insert(name);
        match container {
            ContainerFormat::Struct(fields) => names.extend(fields.iter().map(|f| f.name.as_str())),
            ContainerFormat::Enum(variants) => {
                for variant in variants.values() {
                    names.insert(&variant.name);
                    if let VariantFormat::Struct(fields) = &variant.value {
                        names.extend(fields.iter().map(|f| f.name.as_str()));
                    }
                }
            }
            _ => {}
        }
    }
    let missing: Vec<_> = names
        .into_iter()
        .filter(|name| !hashed.contains(name))
        .collect();
    assert!(missing.is_empty(), "WIRE_ID does not hash {missing:?}");
}
