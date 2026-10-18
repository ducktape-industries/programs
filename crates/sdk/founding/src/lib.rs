//! Packs what `make wasm-modules` builds into a founding: each program with its describe module,
//! its schema, any view and any notifier embedded, forge's and halo's params, and a founding file
//! `ducktape init` reads.

mod schema;

use std::borrow::Cow;
use std::fs;
use std::path::Path;

use abi::role::halo::{Class, Founded, Genesis};
use borsh::BorshDeserialize;
use borsh::schema::BorshSchemaContainer;
use program::schema::roots;
use wasm_encoder::{CustomSection, Encode, Section};
use wasmparser::{Encoding, Parser, Payload, Validator};

pub use schema::{schema, write_schemas};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const VIEW_SECTION: &str = "ducktape.view";
pub const DESCRIBE_SECTION: &str = "ducktape.describe";
/// A program's notifier: the headless module of its view crate the app runs
/// once per network to post the program's notices (`view::Notifier`).
pub const NOTIFY_SECTION: &str = "ducktape.notify";
pub const SCHEMA_SECTION: &str = program::schema::SECTION;
pub const FOUNDING: &str = include_str!("../founding.toml");

pub const PROGRAMS: [(&str, &str); 7] = [
    ("programs", "settings_view"),
    ("valset", "node_view"),
    ("identity", "members_view"),
    ("chat", "chat_view"),
    ("forge", "forge_view"),
    ("harness", "harness_view"),
    ("files", "files_view"),
];
pub const UNVIEWED: &[&str] = &[
    "blobs",
    "awaits",
    "halo",
    "scheduler",
    "access",
    "activity",
    "egress",
    "attribution",
];
pub const VIEWS: [&str; 1] = ["explorer_view"];
/// The programs that ship a notifier, and the module `make wasm-notifiers`
/// builds it as (`<view>.notify.wasm`).
pub const NOTIFIERS: [(&str, &str); 2] = [("chat", "chat_view"), ("forge", "forge_view")];

pub fn forge_bounds() -> forge::Bounds {
    forge::Bounds {
        max_objects: 1_000_000,
        max_delta_depth: 64,
        max_object_size: 256 << 20,
        push_walk: 10_000,
        fetch_walk: 1_000_000,
        merge_cost: 1024,
        page_size: 128,
        log_walk: 10_000,
        tree_walk: 1024,
        diff_bytes: 32 << 20,
        blob_bytes: 256 << 10,
        record_bytes: 64 << 10,
    }
}

/// The lanes halo serves from the first block, given ids in order from 0. blobs' lane is
/// `Critical`: a node that lags fetches the blobs a block names at that class before it can
/// apply the block, and it is pinned, so no vote pauses or closes it: a node cut off from
/// blobs could never apply the block that opened it again. The live relay's lane carries
/// clients' tracks, so it is served no higher than `Client`, and a vote may pause it.
pub fn halo_lanes() -> Genesis {
    Genesis {
        lanes: vec![
            Founded {
                name: "ducktape/blobs".into(),
                cap: Class::Critical,
                pinned: true,
            },
            Founded {
                name: "ducktape/live".into(),
                cap: Class::Client,
                pinned: false,
            },
        ],
    }
}

pub fn bundle(release: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out)?;
    let viewed = PROGRAMS.iter().map(|(program, _)| *program);
    for program in viewed.chain(UNVIEWED.iter().copied()) {
        fs::write(out.join(format!("{program}.wasm")), pack(release, program)?)?;
    }
    for view in VIEWS {
        let bytes = read(release, view, "wasm")?;
        validate(&bytes)?;
        fs::write(out.join(format!("{view}.wasm")), bytes)?;
    }
    fs::write(out.join("forge.params"), abi::encode(&forge_bounds()))?;
    fs::write(out.join("halo.params"), abi::encode(&halo_lanes()))?;
    fs::write(out.join("founding.toml"), FOUNDING)?;
    Ok(())
}

// The view and the notifier a founding packs into `program`, by the module names
// `make wasm-modules` builds them as; `None` for a program no founding holds.
pub fn modules(program: &str) -> Option<(Option<&'static str>, Option<&'static str>)> {
    let notifier = NOTIFIERS
        .iter()
        .find(|(name, _)| *name == program)
        .map(|(_, view)| *view);
    match PROGRAMS.iter().find(|(name, _)| *name == program) {
        Some((_, view)) => Some((Some(*view), notifier)),
        None => UNVIEWED.contains(&program).then_some((None, notifier)),
    }
}

// One program's code as the bundle writes it: what `make wasm-modules` built for it under
// `release`, with its view, describe module, schema and notifier embedded.
pub fn pack(release: &Path, program: &str) -> Result<Vec<u8>> {
    let (view, notifier) =
        modules(program).ok_or_else(|| format!("{program} is not a program a founding packs"))?;
    let mut packed = read(release, program, "wasm")?;
    if let Some(view) = view {
        packed = embed(&packed, &read(release, view, "wasm")?)?;
    }
    let describe = read(release, &format!("{program}.describe"), "wasm")?;
    packed = embed_describe(&packed, &describe)?;
    packed = embed_schema(&packed, &read(release, program, "schema")?)?;
    if let Some(view) = notifier {
        packed = embed_notify(&packed, &read(release, &format!("{view}.notify"), "wasm")?)?;
    }
    Ok(packed)
}

fn read(release: &Path, name: &str, extension: &str) -> Result<Vec<u8>> {
    let path = release.join(format!("{name}.{extension}"));
    Ok(fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?)
}

fn validate(bytes: &[u8]) -> Result<()> {
    let module = matches!(
        Parser::new(0).parse_all(bytes).next(),
        Some(Ok(Payload::Version {
            encoding: Encoding::Module,
            ..
        }))
    );
    if !module {
        return Err("expected a core WebAssembly module".into());
    }
    Validator::new().validate_all(bytes)?;
    Ok(())
}

pub fn strip_section(program: &[u8], name: &str) -> Result<Vec<u8>> {
    validate(program)?;
    let mut output = Vec::with_capacity(program.len());
    let mut keep = 0;
    let mut section_start = 8;
    for payload in Parser::new(0).parse_all(program) {
        let payload = payload?;
        let Some((_, range)) = payload.as_section() else {
            continue;
        };
        let named = matches!(&payload, Payload::CustomSection(s) if s.name() == name);
        if named {
            output.extend_from_slice(&program[keep..section_start]);
            keep = range.end;
        }
        section_start = range.end;
    }
    output.extend_from_slice(&program[keep..]);
    Ok(output)
}

pub fn embed(program: &[u8], view: &[u8]) -> Result<Vec<u8>> {
    validate(view)?;
    embed_section(program, VIEW_SECTION, view)
}

pub fn embed_notify(program: &[u8], notifier: &[u8]) -> Result<Vec<u8>> {
    validate(notifier)?;
    embed_section(program, NOTIFY_SECTION, notifier)
}

pub fn embed_describe(program: &[u8], module: &[u8]) -> Result<Vec<u8>> {
    validate(module)?;
    for payload in Parser::new(0).parse_all(module) {
        if let Payload::ImportSection(imports) = payload?
            && imports.count() > 0
        {
            return Err("a describe module imports nothing".into());
        }
    }
    embed_section(program, DESCRIBE_SECTION, module)
}

pub fn embed_schema(program: &[u8], schema: &[u8]) -> Result<Vec<u8>> {
    let container = BorshSchemaContainer::try_from_slice(schema)?;
    container
        .validate()
        .map_err(|error| format!("the schema is not valid: {error:?}"))?;
    if roots(&container).is_none() {
        return Err("a schema names an op, a query and a reply".into());
    }
    embed_section(program, SCHEMA_SECTION, schema)
}

fn embed_section(program: &[u8], name: &str, payload: &[u8]) -> Result<Vec<u8>> {
    let mut output = strip_section(program, name)?;
    let section = CustomSection {
        name: Cow::Borrowed(name),
        data: Cow::Borrowed(payload),
    };
    output.push(section.id());
    section.encode(&mut output);
    Ok(output)
}
