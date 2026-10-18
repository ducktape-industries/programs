//! Each program's interface schema, from its crate linked natively with `program` off: what
//! `schemas RELEASE` writes as `<program>.schema` and the bundle packs as `ducktape.schema`.
use std::fs;
use std::path::Path;

use borsh::schema::BorshSchemaContainer;
use program::schema::interface;

use crate::Result;

pub fn schema(program: &str) -> Option<BorshSchemaContainer> {
    let schema = match program {
        "programs" => interface::<programs::Programs>(),
        "valset" => interface::<valset::Valset>(),
        "identity" => interface::<identity::Identity>(),
        "blobs" => interface::<blobs::Blobs>(),
        "awaits" => interface::<awaits::Awaits>(),
        "halo" => interface::<halo::Halo>(),
        "scheduler" => interface::<scheduler::Scheduler>(),
        "access" => interface::<access::Access>(),
        "chat" => interface::<chat::Chat>(),
        "forge" => interface::<forge::Forge>(),
        "files" => interface::<files::Files>(),
        "activity" => interface::<activity::Activity>(),
        "egress" => interface::<egress::Egress>(),
        "attribution" => interface::<attribution::Attribution>(),
        "harness" => interface::<harness::Harness>(),
        "actions" => interface::<actions::Actions>(),
        "actions-pods" => interface::<actions_pods::ActionsPods>(),
        _ => return None,
    };
    Some(schema)
}

pub fn write_schemas(release: &Path, programs: &[impl AsRef<str>]) -> Result<()> {
    fs::create_dir_all(release)?;
    for program in programs {
        let program = program.as_ref();
        let schema = schema(program).ok_or_else(|| {
            format!(
                "{program} has no schema: name its interface in crates/sdk/founding/src/schema.rs"
            )
        })?;
        fs::write(
            release.join(format!("{program}.schema")),
            borsh::to_vec(&schema)?,
        )?;
    }
    Ok(())
}
