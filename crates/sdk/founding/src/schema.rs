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

include!(concat!(env!("OUT_DIR"), "/interfaces.rs"));

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
                "{program} has no schema: it is no crate under crates/system or crates/app with a `program` feature"
            )
        })?;
        let (schema, tools) = borsh_json::tools::sections_of(&schema, &tools)
            .map_err(|why| format!("{program}: {why}"))?;
        fs::write(release.join(format!("{program}.schema")), schema)?;
        fs::write(release.join(format!("{program}.tools")), tools)?;
    }
    Ok(())
}
