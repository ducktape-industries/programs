//! Where a program's interface schema comes from: the `ducktape.schema` custom section of the code
//! the programs role runs it from, read out of the code's blob a section header at a time, and
//! what its `ducktape.tools` section tells of its ops and queries.
#[cfg(test)]
pub(crate) mod tests;

use ::program::schema::SECTION;
use ::program::{BlobId, Error, QueryCtx, code, is_id, section, tools};
use abi::role::programs as role;
pub(crate) use borsh_json::Interface;
use borsh_json::tools::Described;

pub(crate) fn schema_of(ctx: &QueryCtx, program: &str) -> Result<Interface, Error> {
    let code = code_of(ctx, program)?;
    schema_in(ctx, program, code)
}

pub(crate) fn schema_in(ctx: &QueryCtx, program: &str, code: BlobId) -> Result<Interface, Error> {
    let Some(section) = section::read(ctx, code, SECTION)? else {
        return Err(Error::new(
            code::UNSUPPORTED,
            format!("program {program}'s code carries no {SECTION} section, so it has no schema"),
        ));
    };
    Interface::read(&section).map_err(|unreadable| {
        Error::new(
            code::CORRUPT,
            format!("program {program}'s {SECTION} section {unreadable}"),
        )
    })
}

// What `program`'s code tells of its ops and queries; nothing where it carries no tools section.
pub(crate) fn told_in(ctx: &QueryCtx, program: &str, code: BlobId) -> Result<Described, Error> {
    let Some(section) = section::read(ctx, code, tools::SECTION)? else {
        return Ok(Described::default());
    };
    borsh_json::tools::read(&section).map_err(|unreadable| {
        Error::new(
            code::CORRUPT,
            format!("program {program}'s code: {unreadable}"),
        )
    })
}

pub(crate) fn code_of(ctx: &QueryCtx, program: &str) -> Result<BlobId, Error> {
    let unknown = || {
        Error::new(
            code::UNKNOWN_PROGRAM,
            format!("no program named {program} runs on this network"),
        )
    };
    if !is_id(program) {
        return Err(unknown());
    }
    running(ctx)?
        .into_iter()
        .find(|entry| entry.program == program)
        .map(|entry| entry.code)
        .ok_or_else(unknown)
}

// Each program that runs at this height, with the code it runs.
pub(crate) fn running(ctx: &QueryCtx) -> Result<Vec<role::Entry>, Error> {
    let programs = ctx.env().role(abi::role::PROGRAMS)?.clone();
    let asked = role::Query::At(ctx.env().height);
    match ctx.query(programs.clone(), &asked)? {
        role::Reply::Programs(entries) => Ok(entries),
        other => Err(::program::unexpected_reply(&programs, "At", &other)),
    }
}
