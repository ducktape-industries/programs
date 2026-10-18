//! How one program interface differs from another: each place where the new op, query, reply or
//! returns read old bytes otherwise than the old did, and each enum that only grew at its end.

use std::collections::BTreeSet;
use std::fmt;

use borsh::schema::{BorshSchemaContainer, Declaration, Definition, Fields};
use program::schema::{Roots, returned, roots};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    // old bytes read the same: a variant added at an enum's end, an op that now names its return
    Appended,
    // old bytes read otherwise, or not at all, or an agent's JSON names it otherwise
    Breaking,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Difference {
    pub at: String,
    pub change: Change,
    pub what: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = match self.change {
            Change::Appended => '+',
            Change::Breaking => '!',
        };
        write!(f, "{mark} {}: {}", self.at, self.what)
    }
}

// Every difference from `old`'s interface to `new`'s, by where it is: `op::Schedule.0.change`.
pub fn differences(
    old: &BorshSchemaContainer,
    new: &BorshSchemaContainer,
) -> crate::Result<Vec<Difference>> {
    let (was, is) = (named(old)?, named(new)?);
    let mut walk = Walk {
        old,
        new,
        seen: BTreeSet::new(),
        found: Vec::new(),
    };
    walk.types("op", was.op, is.op);
    walk.types("query", was.query, is.query);
    walk.types("reply", was.reply, is.reply);
    walk.returns(&was, &is);
    Ok(walk.found)
}

fn named(schema: &BorshSchemaContainer) -> crate::Result<Roots<'_>> {
    Ok(roots(schema).ok_or("a schema names an op, a query and a reply")?)
}

struct Walk<'s> {
    old: &'s BorshSchemaContainer,
    new: &'s BorshSchemaContainer,
    // each pair of types compared once: a type used twice is told of once, a recursive one ends
    seen: BTreeSet<(Declaration, Declaration)>,
    found: Vec<Difference>,
}

impl Walk<'_> {
    fn note(&mut self, at: &str, change: Change, what: String) {
        self.found.push(Difference {
            at: at.to_owned(),
            change,
            what,
        });
    }

    fn types(&mut self, at: &str, was: &str, is: &str) {
        if !self.seen.insert((was.to_owned(), is.to_owned())) {
            return;
        }
        let (old, new) = (self.old, self.new);
        match (old.get_definition(was), new.get_definition(is)) {
            (None, None) if was == is => {}
            (Some(old), Some(new)) => self.definitions(at, (was, old), (is, new)),
            _ => self.note(at, Change::Breaking, format!("was {was}, is {is}")),
        }
    }

    fn definitions(&mut self, at: &str, was: (&str, &Definition), is: (&str, &Definition)) {
        let ((old_name, old), (new_name, new)) = (was, is);
        let differs = || {
            format!(
                "was {old_name} ({}), is {new_name} ({})",
                shape(old),
                shape(new)
            )
        };
        match (old, new) {
            (Definition::Primitive(a), Definition::Primitive(b)) => {
                if a != b || old_name != new_name {
                    self.note(at, Change::Breaking, differs());
                }
            }
            (
                Definition::Sequence {
                    length_width: old_width,
                    length_range: old_range,
                    elements: old_elements,
                },
                Definition::Sequence {
                    length_width: new_width,
                    length_range: new_range,
                    elements: new_elements,
                },
            ) => {
                if old_width != new_width || old_range != new_range {
                    let what = format!("was {old_name}, is {new_name}: its length reads otherwise");
                    self.note(at, Change::Breaking, what);
                } else {
                    self.types(&format!("{at}[]"), old_elements, new_elements);
                }
            }
            (Definition::Tuple { elements: a }, Definition::Tuple { elements: b }) => {
                self.fields(at, unnamed(a), unnamed(b));
            }
            (Definition::Struct { fields: a }, Definition::Struct { fields: b }) => {
                self.fields(at, listed(a), listed(b));
            }
            (
                Definition::Enum {
                    tag_width: old_width,
                    variants: old_variants,
                    ..
                },
                Definition::Enum {
                    tag_width: new_width,
                    variants: new_variants,
                    ..
                },
            ) => {
                if old_width != new_width {
                    let what = format!("its tag was {old_width} bytes, is {new_width}");
                    self.note(at, Change::Breaking, what);
                } else {
                    self.variants(at, old_variants, new_variants);
                }
            }
            _ => self.note(at, Change::Breaking, differs()),
        }
    }

    fn fields(&mut self, at: &str, old: Vec<Field<'_>>, new: Vec<Field<'_>>) {
        let pairs = old.iter().zip(&new).enumerate();
        for (index, ((old_name, old_type), (new_name, new_type))) in pairs {
            let (was, is) = (label(index, *old_name), label(index, *new_name));
            if was != is {
                let what = format!("field {index} was {was}, is {is}");
                self.note(at, Change::Breaking, what);
            }
            self.types(&format!("{at}.{is}"), old_type, new_type);
        }
        for (index, (name, _)) in new.iter().enumerate().skip(old.len()) {
            let what = format!("field {} added", label(index, *name));
            self.note(at, Change::Breaking, what);
        }
        for (index, (name, _)) in old.iter().enumerate().skip(new.len()) {
            let what = format!("field {} is gone", label(index, *name));
            self.note(at, Change::Breaking, what);
        }
    }

    fn variants(&mut self, at: &str, old: &[Variant], new: &[Variant]) {
        for ((old_tag, old_name, old_type), (new_tag, new_name, new_type)) in old.iter().zip(new) {
            if old_tag != new_tag {
                let what = format!("variant {old_name} was tag {old_tag}, is tag {new_tag}");
                self.note(at, Change::Breaking, what);
            } else if old_name != new_name {
                let what = format!("variant {old_tag} was {old_name}, is {new_name}");
                self.note(at, Change::Breaking, what);
            }
            self.types(&format!("{at}::{new_name}"), old_type, new_type);
        }
        for (tag, name, _) in new.iter().skip(old.len()) {
            let what = format!("variant {tag} {name} added at the end");
            self.note(at, Change::Appended, what);
        }
        for (tag, name, _) in old.iter().skip(new.len()) {
            let what = format!("variant {tag} {name} is gone");
            self.note(at, Change::Breaking, what);
        }
    }

    fn returns(&mut self, was: &Roots<'_>, is: &Roots<'_>) {
        let (old, new) = (self.old, self.new);
        let old = was
            .returns
            .and_then(|r| returned(old, r))
            .unwrap_or_default();
        let new = is
            .returns
            .and_then(|r| returned(new, r))
            .unwrap_or_default();
        for (op, of) in &new {
            match old.iter().find(|(returning, _)| returning == op) {
                Some((_, was_of)) => self.types(&format!("returns::{op}"), was_of, of),
                None => {
                    let what = format!("op {op} now says it returns {of}");
                    self.note("returns", Change::Appended, what);
                }
            }
        }
        for (op, of) in &old {
            if !new.iter().any(|(returning, _)| returning == op) {
                let what = format!("op {op} no longer says it returns {of}");
                self.note("returns", Change::Breaking, what);
            }
        }
    }
}

type Field<'d> = (Option<&'d str>, &'d Declaration);
type Variant = (i64, String, Declaration);

fn label(index: usize, name: Option<&str>) -> String {
    name.map_or_else(|| index.to_string(), str::to_owned)
}

fn unnamed(elements: &[Declaration]) -> Vec<Field<'_>> {
    elements.iter().map(|of| (None, of)).collect()
}

fn listed(fields: &Fields) -> Vec<Field<'_>> {
    match fields {
        Fields::NamedFields(named) => named
            .iter()
            .map(|(name, of)| (Some(name.as_str()), of))
            .collect(),
        Fields::UnnamedFields(unnamed) => unnamed.iter().map(|of| (None, of)).collect(),
        Fields::Empty => Vec::new(),
    }
}

fn shape(definition: &Definition) -> &'static str {
    match definition {
        Definition::Primitive(_) => "a fixed-size value",
        Definition::Sequence { .. } => "a sequence",
        Definition::Tuple { .. } => "a tuple",
        Definition::Struct { .. } => "a struct",
        Definition::Enum { .. } => "an enum",
    }
}
