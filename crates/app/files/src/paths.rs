//! Paths as files spells them (NFC, absolute, `/`-separated), and the namespace roots a network
//! names in files' params: each a top folder and the folders `depth` levels beneath it, which the
//! chain alone writes, owned by the account each is named after or by one program.
use ::program::{Error, QueryCtx, invalid, unauthorized, unexpected_reply};
use borsh::{BorshDeserialize, BorshSerialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Root {
    pub name: String,
    pub depth: u8,
    pub owner: Option<Owner>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Owner {
    Named,
    Program(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roots(Vec<Root>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keeper<'r> {
    Named(&'r str),
    Program(&'r str),
}

impl Roots {
    pub fn new(roots: Vec<Root>) -> Roots {
        Roots(roots)
    }

    pub fn of(params: &[u8]) -> Result<Roots, Error> {
        if params.is_empty() {
            return Ok(Roots::default());
        }
        abi::decode(params).map(Roots).map_err(|_| {
            invalid("files' params do not read as its roots: a borsh Vec of Root { name, depth, owner }")
        })
    }

    pub fn read(ctx: &QueryCtx) -> Result<Roots, Error> {
        let role = ctx.env().role(abi::role::PROGRAMS)?.clone();
        let asked = programs::Query::Program(ctx.env().program.clone());
        match ctx.query::<programs::Query, programs::Reply>(&role, &asked)? {
            programs::Reply::Program { entry, .. } => match entry {
                Some(entry) => Roots::of(&entry.params),
                None => Ok(Roots::default()),
            },
            other => Err(unexpected_reply(&role, "Program", &other)),
        }
    }

    fn root(&self, segments: &[String]) -> Option<(&Root, usize)> {
        let (top, below) = segments.split_first()?;
        let root = self.0.iter().find(|root| root.name == *top)?;
        Some((root, below.len()))
    }

    pub fn is_namespace_root(&self, segments: &[String]) -> bool {
        self.root(segments)
            .is_some_and(|(root, below)| below <= usize::from(root.depth))
    }

    pub fn holds_namespace_roots(&self, folder: &[String]) -> bool {
        self.root(folder)
            .is_some_and(|(root, below)| below < usize::from(root.depth))
    }

    pub fn keeper<'s>(&'s self, segments: &'s [String]) -> Option<Keeper<'s>> {
        let (root, below) = self.root(segments)?;
        let chained = below <= usize::from(root.depth);
        let named = below == 1;
        match (&root.owner, chained) {
            (Some(Owner::Named), true) => segments
                .last()
                .filter(|_| named)
                .map(|name| Keeper::Named(name.as_str())),
            (Some(Owner::Program(program)), true) => Some(Keeper::Program(program)),
            (Some(Owner::Named | Owner::Program(_)) | None, _) => None,
        }
    }

    pub fn carried(&self, segments: &[String]) -> bool {
        self.root(segments).is_some_and(|(root, below)| {
            let kept = matches!(root.owner, Some(Owner::Program(_)));
            kept && below > usize::from(root.depth)
        })
    }

    pub(crate) fn beneath(&self, segments: &[String]) -> Result<(), Error> {
        if !self.is_namespace_root(segments) {
            return Ok(());
        }
        Err(unauthorized(format!(
            "{} is a namespace root, which files' params name and the chain alone writes; anyone writes beneath it where no write list says otherwise: write beneath it instead",
            join(segments)
        )))
    }
}

pub fn segments(path: &str) -> Result<Vec<String>, Error> {
    if !path.starts_with('/') {
        return Err(invalid(format!(
            "{path:?} is not absolute: a path starts with /"
        )));
    }
    if path.contains('\0') {
        return Err(invalid("a path holds no NUL byte"));
    }
    let composed = path.nfc().eq(path.chars());
    if !composed {
        return Err(invalid(format!("{path:?} is not in NFC form")));
    }
    if path == "/" {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for segment in path[1..].split('/') {
        let navigates = segment.is_empty() || segment == "." || segment == "..";
        if navigates {
            return Err(invalid(format!("{path:?} has an empty, . or .. segment")));
        }
        segments.push(segment.to_owned());
    }
    Ok(segments)
}

pub fn join(segments: &[String]) -> String {
    format!("/{}", segments.join("/"))
}

pub fn change_path(path: &str) -> Result<Vec<String>, Error> {
    let segments = segments(path)?;
    if segments.is_empty() {
        return Err(invalid(
            "the root is not a path a change names: name a path beneath the root",
        ));
    }
    Ok(segments)
}

#[cfg(test)]
mod tests;
