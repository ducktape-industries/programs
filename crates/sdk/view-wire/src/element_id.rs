//! Wire representation of GPUI element identity.
//!
//! The wire identity keeps the GPUI tag instead of reducing an ID to a string.
//! `NamedChild` is stored as one atom plus a bounded list of names. This keeps
//! decoding iterative and prevents attacker-controlled recursive allocation.

use crate::codec::bin::{bounded, fixed};
use gpui::{ElementId, EntityId, FocusId, SharedString};
use serde::{Deserialize, Deserializer, Serialize};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

/// The maximum number of named-child components accepted in one element ID.
pub const MAX_ELEMENT_ID_DEPTH: usize = 64;

/// The non-recursive base of a GPUI element ID.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub enum ElementIdAtom {
    View(u64),
    Integer(u64),
    Name(SharedString),
    Uuid(#[serde(serialize_with = "crate::codec::bin::serialize_fixed")] [u8; 16]),
    FocusHandle(u64),
    NamedInteger(SharedString, u64),
    Path(#[serde(with = "crate::codec::bin")] Vec<u8>),
    CodeLocation {
        file: String,
        line: u32,
        column: u32,
    },
    OpaqueId(#[serde(serialize_with = "crate::codec::bin::serialize_fixed")] [u8; 20]),
}

/// A tagged, lossless wire form of GPUI's element identity.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub enum ElementIdWire {
    View(u64),
    Integer(u64),
    Name(SharedString),
    Uuid(#[serde(serialize_with = "crate::codec::bin::serialize_fixed")] [u8; 16]),
    FocusHandle(u64),
    NamedInteger(SharedString, u64),
    /// UTF-8 path bytes. Non-UTF-8 paths are rejected at the GPUI boundary.
    Path(#[serde(with = "crate::codec::bin")] Vec<u8>),
    CodeLocation {
        file: String,
        line: u32,
        column: u32,
    },
    NamedChild {
        base: ElementIdAtom,
        names: Vec<SharedString>,
    },
    OpaqueId(#[serde(serialize_with = "crate::codec::bin::serialize_fixed")] [u8; 20]),
}

impl<'de> Deserialize<'de> for ElementIdAtom {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename = "ElementIdAtom")]
        enum AtomRepr {
            View(u64),
            Integer(u64),
            Name(SharedString),
            Uuid(#[serde(deserialize_with = "uuid_bytes")] Vec<u8>),
            FocusHandle(u64),
            NamedInteger(SharedString, u64),
            Path(#[serde(with = "crate::codec::bin")] Vec<u8>),
            CodeLocation {
                file: String,
                line: u32,
                column: u32,
            },
            OpaqueId(#[serde(deserialize_with = "opaque_id_bytes")] Vec<u8>),
        }

        Ok(match AtomRepr::deserialize(deserializer)? {
            AtomRepr::View(id) => Self::View(id),
            AtomRepr::Integer(id) => Self::Integer(id),
            AtomRepr::Name(name) => Self::Name(name),
            AtomRepr::Uuid(id) => Self::Uuid(fixed(id)?),
            AtomRepr::FocusHandle(id) => Self::FocusHandle(id),
            AtomRepr::NamedInteger(name, id) => Self::NamedInteger(name, id),
            AtomRepr::Path(path) => Self::Path(path),
            AtomRepr::CodeLocation { file, line, column } => {
                Self::CodeLocation { file, line, column }
            }
            AtomRepr::OpaqueId(id) => Self::OpaqueId(fixed(id)?),
        })
    }
}

impl<'de> Deserialize<'de> for ElementIdWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        enum WireRepr {
            View(u64),
            Integer(u64),
            Name(SharedString),
            Uuid(#[serde(deserialize_with = "uuid_bytes")] Vec<u8>),
            FocusHandle(u64),
            NamedInteger(SharedString, u64),
            Path(#[serde(with = "crate::codec::bin")] Vec<u8>),
            CodeLocation {
                file: String,
                line: u32,
                column: u32,
            },
            NamedChild {
                base: ElementIdAtom,
                #[serde(deserialize_with = "deserialize_names")]
                names: Vec<SharedString>,
            },
            OpaqueId(#[serde(deserialize_with = "opaque_id_bytes")] Vec<u8>),
        }

        Ok(match WireRepr::deserialize(deserializer)? {
            WireRepr::View(id) => Self::View(id),
            WireRepr::Integer(id) => Self::Integer(id),
            WireRepr::Name(name) => Self::Name(name),
            WireRepr::Uuid(id) => Self::Uuid(fixed(id)?),
            WireRepr::FocusHandle(id) => Self::FocusHandle(id),
            WireRepr::NamedInteger(name, id) => Self::NamedInteger(name, id),
            WireRepr::Path(path) => Self::Path(path),
            WireRepr::CodeLocation { file, line, column } => {
                Self::CodeLocation { file, line, column }
            }
            WireRepr::NamedChild { base, names } => Self::NamedChild { base, names },
            WireRepr::OpaqueId(id) => Self::OpaqueId(fixed(id)?),
        })
    }
}

fn uuid_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    bounded(deserializer, 16, "a Uuid is 16 bytes")
}

fn opaque_id_bytes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    bounded(deserializer, 20, "an OpaqueId is 20 bytes")
}

fn deserialize_names<'de, D>(deserializer: D) -> Result<Vec<SharedString>, D::Error>
where
    D: Deserializer<'de>,
{
    crate::bounded_vec(
        deserializer,
        MAX_ELEMENT_ID_DEPTH,
        "named-child identity is too deep",
    )
}

impl ElementIdWire {
    /// Convert an authoring ID without erasing its GPUI variant.
    pub fn from_gpui(id: ElementId) -> Result<Self, &'static str> {
        let mut names = Vec::new();
        let mut current = id;
        let base = loop {
            match current {
                ElementId::NamedChild(parent, name) => {
                    if names.len() == MAX_ELEMENT_ID_DEPTH {
                        return Err("named-child identity is too deep");
                    }
                    names.push(name);
                    current = (*parent).clone();
                }
                base => break ElementIdAtom::from_gpui(base)?,
            }
        };

        if names.is_empty() {
            Ok(Self::from_atom(base))
        } else {
            names.reverse();
            Ok(Self::NamedChild { base, names })
        }
    }

    /// Lower an ID to native GPUI, rejecting IDs that cannot be reconstructed
    /// in the host process without changing their identity.
    pub fn to_gpui(&self) -> Result<ElementId, &'static str> {
        self.validate_host()?;
        match self {
            Self::NamedChild { base, names } => {
                let mut id = base.to_gpui()?;
                for name in names {
                    id = ElementId::NamedChild(Arc::new(id), name.clone());
                }
                Ok(id)
            }
            _ => self.atom().expect("an atom").to_gpui(),
        }
    }

    /// Reject host-unsupported IDs before the renderer can invent a fallback.
    pub fn validate_host(&self) -> Result<(), &'static str> {
        match self {
            Self::NamedChild { base, names } => {
                if names.is_empty() || names.len() > MAX_ELEMENT_ID_DEPTH {
                    return Err("named-child identity has an invalid depth");
                }
                base.validate_host()
            }
            _ => self.atom().expect("an atom").validate_host(),
        }
    }

    fn atom(&self) -> Option<ElementIdAtom> {
        Some(match self {
            Self::View(id) => ElementIdAtom::View(*id),
            Self::Integer(id) => ElementIdAtom::Integer(*id),
            Self::Name(name) => ElementIdAtom::Name(name.clone()),
            Self::Uuid(id) => ElementIdAtom::Uuid(*id),
            Self::FocusHandle(id) => ElementIdAtom::FocusHandle(*id),
            Self::NamedInteger(name, id) => ElementIdAtom::NamedInteger(name.clone(), *id),
            Self::Path(path) => ElementIdAtom::Path(path.clone()),
            Self::CodeLocation { file, line, column } => ElementIdAtom::CodeLocation {
                file: file.clone(),
                line: *line,
                column: *column,
            },
            Self::OpaqueId(id) => ElementIdAtom::OpaqueId(*id),
            Self::NamedChild { .. } => return None,
        })
    }

    /// The name of a `Name` id: the key paths and accessibility faults use.
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::Name(name) => Some(name.as_ref()),
            _ => None,
        }
    }

    fn from_atom(atom: ElementIdAtom) -> Self {
        match atom {
            ElementIdAtom::View(id) => Self::View(id),
            ElementIdAtom::Integer(id) => Self::Integer(id),
            ElementIdAtom::Name(name) => Self::Name(name),
            ElementIdAtom::Uuid(id) => Self::Uuid(id),
            ElementIdAtom::FocusHandle(id) => Self::FocusHandle(id),
            ElementIdAtom::NamedInteger(name, id) => Self::NamedInteger(name, id),
            ElementIdAtom::Path(path) => Self::Path(path),
            ElementIdAtom::CodeLocation { file, line, column } => {
                Self::CodeLocation { file, line, column }
            }
            ElementIdAtom::OpaqueId(id) => Self::OpaqueId(id),
        }
    }
}

impl ElementIdAtom {
    fn from_gpui(id: ElementId) -> Result<Self, &'static str> {
        Ok(match id {
            ElementId::View(id) => Self::View(id.as_u64()),
            ElementId::Integer(id) => Self::Integer(id),
            ElementId::Name(id) => Self::Name(id),
            ElementId::Uuid(id) => Self::Uuid(*id.as_bytes()),
            ElementId::FocusHandle(id) => Self::FocusHandle(slotmap::Key::data(&id).as_ffi()),
            ElementId::NamedInteger(name, id) => Self::NamedInteger(name, id),
            ElementId::Path(path) => Self::Path(
                path.to_str()
                    .ok_or("non-UTF-8 element paths cannot cross the wire")?
                    .as_bytes()
                    .to_vec(),
            ),
            ElementId::CodeLocation(location) => Self::CodeLocation {
                file: location.file().into(),
                line: location.line(),
                column: location.column(),
            },
            ElementId::NamedChild(_, _) => {
                return Err("named-child must be flattened before atom conversion");
            }
            ElementId::OpaqueId(id) => Self::OpaqueId(id),
        })
    }

    fn to_gpui(&self) -> Result<ElementId, &'static str> {
        Ok(match self {
            Self::View(id) => ElementId::View(EntityId::from(*id)),
            Self::Integer(id) => ElementId::Integer(*id),
            Self::Name(name) => ElementId::Name(name.clone()),
            Self::Uuid(id) => ElementId::Uuid(Uuid::from_bytes(*id)),
            Self::FocusHandle(id) => {
                ElementId::FocusHandle(FocusId::from(slotmap::KeyData::from_ffi(*id)))
            }
            Self::NamedInteger(name, id) => ElementId::NamedInteger(name.clone(), *id),
            Self::Path(path) => ElementId::Path(Arc::from(PathBuf::from(
                std::str::from_utf8(path).map_err(|_| "element path is not UTF-8")?,
            ))),
            Self::CodeLocation { .. } => {
                return Err("code-location element IDs cannot cross the wire");
            }
            Self::OpaqueId(id) => ElementId::OpaqueId(*id),
        })
    }

    fn validate_host(&self) -> Result<(), &'static str> {
        match self {
            Self::FocusHandle(_) => Err("focus-handle element IDs are host-local"),
            Self::CodeLocation { .. } => Err("code-location element IDs cannot cross the wire"),
            Self::Path(path) if std::str::from_utf8(path).is_err() => {
                Err("element path is not UTF-8")
            }
            _ => Ok(()),
        }
    }
}
