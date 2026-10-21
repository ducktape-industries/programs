//! Where a program's interface schema comes from: the `ducktape.schema` custom section of the code
//! the programs role runs it from, read out of the code's blob a section header at a time, and
//! what its `ducktape.tools` section tells of its ops and queries.
#[cfg(test)]
pub(crate) mod tests;

use ::program::schema::SECTION;
use ::program::{BlobId, Ctx, Error, ExecCtx, QueryCtx, code, is_id, section, tools};
use abi::role::programs as role;
pub(crate) use borsh_json::Interface;
use borsh_json::tools::Described;

use crate::state::{SECTIONS, SectionRow};

pub(crate) trait Keeps {
    fn query_ctx(&self) -> &QueryCtx;
    fn keep(&self, key: &(String, String), row: &SectionRow);
}

impl Keeps for QueryCtx {
    fn query_ctx(&self) -> &QueryCtx {
        self
    }

    fn keep(&self, _: &(String, String), _: &SectionRow) {}
}

impl Keeps for ExecCtx {
    fn query_ctx(&self) -> &QueryCtx {
        self
    }

    fn keep(&self, key: &(String, String), row: &SectionRow) {
        SECTIONS.put(self, key, row);
    }
}

impl Keeps for Ctx {
    fn query_ctx(&self) -> &QueryCtx {
        self
    }

    fn keep(&self, key: &(String, String), row: &SectionRow) {
        SECTIONS.put(self, key, row);
    }
}

fn kept_section(
    ctx: &dyn Keeps,
    program: &str,
    code: BlobId,
    name: &str,
) -> Result<Option<Vec<u8>>, Error> {
    let key = (program.to_owned(), name.to_owned());
    let kept = SECTIONS
        .get(ctx.query_ctx(), &key)?
        .filter(|row| row.code == code);
    if let Some(row) = kept {
        return Ok(row.section);
    }
    let section = section::read(ctx.query_ctx(), code, name)?;
    let row = SectionRow {
        code,
        section: section.clone(),
    };
    ctx.keep(&key, &row);
    Ok(section)
}

pub(crate) fn schema_of(ctx: &dyn Keeps, program: &str) -> Result<Interface, Error> {
    let code = code_of(ctx.query_ctx(), program)?;
    schema_in(ctx, program, code)
}

pub(crate) fn schema_in(ctx: &dyn Keeps, program: &str, code: BlobId) -> Result<Interface, Error> {
    let Some(section) = kept_section(ctx, program, code, SECTION)? else {
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
pub(crate) fn told_in(ctx: &dyn Keeps, program: &str, code: BlobId) -> Result<Described, Error> {
    let Some(section) = kept_section(ctx, program, code, tools::SECTION)? else {
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
