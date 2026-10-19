//! Where a program's interface schema comes from: the `ducktape.schema` custom section of the code
//! the programs role runs it from, read out of the code's blob a section header at a time.
#[cfg(test)]
pub(crate) mod tests;

use ::program::schema::SECTION;
use ::program::{BlobId, Error, QueryCtx, code, is_name, section};
use abi::role::programs as role;
pub(crate) use borsh_json::Interface;

pub(crate) fn schema_of(ctx: &QueryCtx, program: &str) -> Result<Interface, Error> {
    let code = code_of(ctx, program)?;
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

pub(crate) fn code_of(ctx: &QueryCtx, program: &str) -> Result<BlobId, Error> {
    let unknown = || {
        Error::new(
            code::UNKNOWN_PROGRAM,
            format!("no program named {program} runs on this network"),
        )
    };
    if !is_name(program) {
        return Err(unknown());
    }
    let programs = ctx.env().role(abi::role::PROGRAMS)?.clone();
    let asked = role::Query::At(ctx.env().height);
    let entries = match ctx.query(programs.clone(), &asked)? {
        role::Reply::Programs(entries) => entries,
        other => return Err(::program::unexpected_reply(&programs, "At", &other)),
    };
    entries
        .into_iter()
        .find(|entry| entry.program == program)
        .map(|entry| entry.code)
        .ok_or_else(unknown)
}
