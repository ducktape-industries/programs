//! Each program's interface schema and what it tells agents (`Program::tools`), from its crate
//! linked natively with `program` off: what `schemas RELEASE` writes as `<program>.schema` and
//! `<program>.tools`, which the bundle packs as `ducktape.schema` and `ducktape.tools`.
use std::fs;
use std::path::Path;

use borsh::BorshSchema;
use borsh::schema::BorshSchemaContainer;
use program::Program;
use program::tools::Tools;

use crate::Result;

pub fn schema(program: &str) -> Option<BorshSchemaContainer> {
    interface(program).map(|(schema, _)| schema)
}

pub fn tools(program: &str) -> Option<Tools> {
    interface(program).map(|(_, tools)| tools)
}

fn interface(program: &str) -> Option<(BorshSchemaContainer, Tools)> {
    let interface = match program {
        "programs" => of::<programs::Programs>(),
        "valset" => of::<valset::Valset>(),
        "identity" => of::<identity::Identity>(),
        "blobs" => of::<blobs::Blobs>(),
        "awaits" => of::<awaits::Awaits>(),
        "halo" => of::<halo::Halo>(),
        "scheduler" => of::<scheduler::Scheduler>(),
        "access" => of::<access::Access>(),
        "ownership" => of::<ownership::Ownership>(),
        "chat" => of::<chat::Chat>(),
        "forge" => of::<forge::Forge>(),
        "files" => of::<files::Files>(),
        "activity" => of::<activity::Activity>(),
        "egress" => of::<egress::Egress>(),
        "attribution" => of::<attribution::Attribution>(),
        "harness" => of::<harness::Harness>(),
        "actions" => of::<actions::Actions>(),
        "actions-pods" => of::<actions_pods::ActionsPods>(),
        "fleet" => of::<fleet::Fleet>(),
        "wake" => of::<wake::Wake>(),
        _ => return None,
    };
    Some(interface)
}

fn of<P: Program>() -> (BorshSchemaContainer, Tools)
where
    P::Op: BorshSchema,
    P::Query: BorshSchema,
    P::Reply: BorshSchema,
{
    (program::schema::interface::<P>(), P::tools())
}

pub fn write_schemas(release: &Path, programs: &[impl AsRef<str>]) -> Result<()> {
    fs::create_dir_all(release)?;
    for program in programs {
        let program = program.as_ref();
        let (schema, tools) = interface(program).ok_or_else(|| {
            format!(
                "{program} has no schema: name its interface in crates/sdk/founding/src/schema.rs"
            )
        })?;
        let schema = borsh::to_vec(&schema)?;
        let read = borsh_json::Interface::read(&schema)
            .map_err(|why| format!("{program}'s schema {why}"))?;
        let tools = borsh_json::tools::section(&read, &tools)
            .map_err(|why| format!("{program}'s tools: {why}"))?;
        fs::write(release.join(format!("{program}.schema")), schema)?;
        fs::write(release.join(format!("{program}.tools")), tools)?;
    }
    Ok(())
}
