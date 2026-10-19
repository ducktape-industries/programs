//! What an op means to a person, as the program that runs it says.
//!
//! A program ships a standalone wasm module in the custom section
//! [`SECTION`] of its code blob, beside `ducktape.view`; the kernel ignores
//! both. The module has NO imports and exports one pure function:
//!
//! ```text
//! export memory
//! export alloc(len: u32) -> u32
//! export describe(ptr: u32, len: u32) -> u64   // (ptr << 32) | len
//! ```
//!
//! `describe` reads the borsh op bytes the host wrote into an `alloc`ed
//! buffer and answers a borsh [`Description`]; `0` (an empty answer) when
//! the bytes are no op of its. A program writes `describe(&Op) ->
//! Description` and one [`export!`] line; its `describe` feature builds the
//! module (`make wasm-describes`).
//!
//! [`Value`] is a small vocabulary, so a reader (Explorer) can show an
//! account as a name, a program as a link, a hash short.
//!
//! VERSIONS: a host describes with the program's CURRENT code, whatever
//! height the op landed at. Old op bytes therefore have to read the same
//! under new code: an `Op` enum only grows at its end, which each
//! program's test holds over [`variants`]. An op the module cannot read
//! is `None`, and the reader falls back to its bytes.
use borsh::{BorshDeserialize, BorshSerialize};

/// The custom section of a program's code blob that carries the module.
pub const SECTION: &str = "ducktape.describe";

/// An op as a person reads it: what it does, and its parts.
#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Description {
    /// `Post in #design`
    pub title: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Field {
    pub label: String,
    pub value: Value,
}

/// What a value is, so a reader shows it as that.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Value {
    Text(String),
    /// an identity account number
    Account(u64),
    /// a public key, which may belong to an account
    Key(Vec<u8>),
    /// a program by name
    Program(String),
    /// a digest: a blob id, a commit
    Hash(Vec<u8>),
    /// `value / 10^decimals`
    Amount {
        value: u128,
        decimals: u8,
    },
    /// milliseconds since the epoch
    Time(u64),
    /// bytes too long to carry: their length and [`Value::bytes`]' preview
    Bytes {
        len: u64,
        preview: Vec<u8>,
    },
    List(#[borsh(deserialize_with = "shallow")] Vec<Value>),
}

impl Value {
    // Bytes as their length and a preview: all of them up to 32, else the
    // first 8 and the last 2 (what `design::preview` shows).
    pub fn bytes(bytes: &[u8]) -> Value {
        let preview = match bytes.len() {
            0..=32 => bytes.to_vec(),
            len => [&bytes[..8], &bytes[len - 2..]].concat(),
        };
        Value::Bytes {
            len: bytes.len() as u64,
            preview,
        }
    }

    pub fn text(text: impl Into<String>) -> Value {
        Value::Text(text.into())
    }

    /// A plain count (an index, a size): an [`Value::Amount`] with no
    /// decimals, which is how a reader shows an integer.
    pub fn count(n: u64) -> Value {
        Value::Amount {
            value: u128::from(n),
            decimals: 0,
        }
    }
}

pub fn field(label: &str, value: Value) -> Field {
    Field {
        label: label.into(),
        value,
    }
}

/// How deep a [`Value::List`] may nest. Decoding refuses deeper, so an
/// untrusted answer cannot recurse the reader's stack away.
pub const MAX_DEPTH: u32 = 4;

std::thread_local! {
    static DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

struct Deeper;

impl Deeper {
    fn enter() -> std::io::Result<Self> {
        DEPTH.with(|depth| {
            if depth.get() >= MAX_DEPTH {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "a description nests too deep",
                ));
            }
            depth.set(depth.get() + 1);
            Ok(Deeper)
        })
    }
}

impl Drop for Deeper {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get() - 1));
    }
}

fn shallow<R: std::io::Read>(reader: &mut R) -> std::io::Result<Vec<Value>> {
    let _deeper = Deeper::enter()?;
    Vec::<Value>::deserialize_reader(reader)
}

impl Description {
    /// Exactly one description, nothing after it, lists at most
    /// [`MAX_DEPTH`] deep.
    pub fn decode(bytes: &[u8]) -> Option<Description> {
        borsh::from_slice(bytes).ok()
    }
}

/// The module's work, over plain bytes: the op decoded as `T` and described,
/// borsh; `None` when the bytes are no `T`.
pub fn describe_bytes<T: BorshDeserialize>(
    op: &[u8],
    describe: fn(&T) -> Description,
) -> Option<Vec<u8>> {
    let op = borsh::from_slice::<T>(op).ok()?;
    Some(borsh::to_vec(&describe(&op)).expect("borsh encodes an in-memory value"))
}

/// The module's exports, for [`export!`].
#[doc(hidden)]
pub mod exports {
    use super::*;

    pub fn alloc(len: u32) -> u32 {
        let mut buffer = Vec::<u8>::with_capacity(len as usize);
        let ptr = buffer.as_mut_ptr();
        std::mem::forget(buffer);
        ptr as usize as u32
    }

    /// # Safety
    /// `ptr..ptr + len` is a buffer [`alloc`] handed out, written by the host.
    pub unsafe fn describe<T: BorshDeserialize>(
        ptr: u32,
        len: u32,
        describe: fn(&T) -> Description,
    ) -> u64 {
        // SAFETY: the caller's contract
        let op = unsafe { std::slice::from_raw_parts(ptr as usize as *const u8, len as usize) };
        match describe_bytes(op, describe) {
            // an instance answers once: the host throws it away after
            Some(answer) => {
                let answer = answer.leak();
                ((answer.as_ptr() as usize as u64) << 32) | answer.len() as u64
            }
            None => 0,
        }
    }
}

/// The module's `alloc` and `describe` exports over `describe(&Op) ->
/// Description`. They exist only in a wasm build with the calling crate's
/// `describe` feature on — the one build that is the module; a program or
/// view build of the same crate has its own `alloc`.
#[macro_export]
macro_rules! export {
    ($op:ty, $describe:path) => {
        #[cfg(all(target_family = "wasm", feature = "describe"))]
        const _: () = {
            // named apart from the caller's `describe`, which `$describe`
            // names inside this block
            #[unsafe(export_name = "alloc")]
            extern "C" fn __describe_alloc(len: u32) -> u32 {
                $crate::exports::alloc(len)
            }
            #[unsafe(export_name = "describe")]
            unsafe extern "C" fn __describe_export(ptr: u32, len: u32) -> u64 {
                // SAFETY: the host writes the op into a buffer `alloc` gave it
                unsafe { $crate::exports::describe::<$op>(ptr, len, $describe) }
            }
        };
    };
}

/// THE VERSION RULE, as a test reads it: an op enum's variant names in tag
/// order. Each variant is decoded from its tag and zero bytes (an empty
/// string, a zero, `None`, the first variant), and named by its `Debug`. A
/// program commits this list; a variant inserted, removed or moved changes
/// a committed name, and only an append passes.
pub fn variants<T: BorshDeserialize + std::fmt::Debug>() -> Vec<String> {
    let zeros = [0u8; 256];
    (0..=u8::MAX)
        .map_while(|tag| {
            let bytes = [&[tag][..], &zeros].concat();
            let op = T::deserialize(&mut &bytes[..]).ok()?;
            let debug = format!("{op:?}");
            debug
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .map(str::to_owned)
        })
        .collect()
}

/// The payload of the describe section of a program's code blob, if it has
/// one. Walks the section headers only; a malformed module has none. Feature-free:
/// a host with no wasm runtime of its own (one that runs the module elsewhere)
/// still reads the section here.
pub fn section(module: &[u8]) -> Option<&[u8]> {
    let mut rest = module.strip_prefix(b"\0asm\x01\0\0\0")?;
    while let Some((&id, after)) = rest.split_first() {
        let (size, after) = leb(after)?;
        let body = after.get(..size)?;
        rest = &after[size..];
        if id == 0 {
            let (len, name) = leb(body)?;
            if name.get(..len)? == SECTION.as_bytes() {
                return Some(&name[len..]);
            }
        }
    }
    None
}

/// An unsigned LEB128 u32 and what follows it.
fn leb(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let mut value = 0u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f).checked_shl(7 * index as u32)?;
        if byte & 0x80 == 0 {
            return Some((value as usize, &bytes[index + 1..]));
        }
    }
    None
}

#[cfg(any(feature = "host", test))]
pub mod host;

#[cfg(test)]
mod tests;
