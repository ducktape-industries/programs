//! A program's tables as the kernel's ownership guard reads them: each one a key prefix that is
//! shared (the program's own code alone governs it) or owned, of a kind. `export!` writes
//! `Program::TABLES` as the module's `ducktape.tables` section; a key under no table is owned.
use abi::role::ownership::{self, TableKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Shared,
    // owned, of the kind a user's defaults name it by, or of none
    Owned(Option<&'static str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Table {
    pub prefix: &'static str,
    pub held: Held,
}

impl Table {
    pub const fn shared(prefix: &'static str) -> Table {
        Table {
            prefix,
            held: Held::Shared,
        }
    }

    pub const fn owned(prefix: &'static str, kind: &'static str) -> Table {
        Table {
            prefix,
            held: Held::Owned(Some(kind)),
        }
    }
}

// The tables the kernel is told of. An owned table of no kind is what a key under no table is
// already, so it is left out.
pub fn declared(tables: &[Table]) -> Vec<ownership::Table> {
    tables
        .iter()
        .filter_map(|table| {
            let kind = match table.held {
                Held::Shared => TableKind::Shared,
                Held::Owned(Some(kind)) => TableKind::Owned(kind.to_owned()),
                Held::Owned(None) => return None,
            };
            let prefix = table.prefix.as_bytes().to_vec();
            Some(ownership::Table { prefix, kind })
        })
        .collect()
}

// The section's length in bytes: the borsh of `declared(tables)`. A prefix declared twice fails
// the build, since the kernel could read either.
pub const fn section_len(tables: &[Table]) -> usize {
    let mut len = 4;
    let mut i = 0;
    while i < tables.len() {
        let mut j = i + 1;
        while j < tables.len() {
            let twice = same(tables[i].prefix.as_bytes(), tables[j].prefix.as_bytes());
            assert!(!twice, "a program declares one table prefix twice");
            j += 1;
        }
        let prefix = 4 + tables[i].prefix.len();
        len += match tables[i].held {
            Held::Shared => prefix + 1,
            Held::Owned(Some(kind)) => prefix + 1 + 4 + kind.len(),
            Held::Owned(None) => 0,
        };
        i += 1;
    }
    len
}

// The section's bytes, written at compile time: borsh `Vec<ownership::Table>`, as `declared`.
pub const fn section<const N: usize>(tables: &[Table]) -> [u8; N] {
    let mut out = [0; N];
    let mut count = 0;
    let mut i = 0;
    while i < tables.len() {
        if !matches!(tables[i].held, Held::Owned(None)) {
            count += 1;
        }
        i += 1;
    }
    let mut at = put_u32(&mut out, 0, count);
    let mut i = 0;
    while i < tables.len() {
        let table = tables[i];
        match table.held {
            Held::Shared => {
                at = put_bytes(&mut out, at, table.prefix.as_bytes());
                out[at] = 0;
                at += 1;
            }
            Held::Owned(Some(kind)) => {
                at = put_bytes(&mut out, at, table.prefix.as_bytes());
                out[at] = 1;
                at = put_bytes(&mut out, at + 1, kind.as_bytes());
            }
            Held::Owned(None) => {}
        }
        i += 1;
    }
    assert!(at == N, "the section is not section_len's length");
    out
}

const fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn put_u32(out: &mut [u8], at: usize, n: u32) -> usize {
    let bytes = n.to_le_bytes();
    let mut i = 0;
    while i < 4 {
        out[at + i] = bytes[i];
        i += 1;
    }
    at + 4
}

const fn put_bytes(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let mut at = put_u32(out, at, bytes.len() as u32);
    let mut i = 0;
    while i < bytes.len() {
        out[at] = bytes[i];
        at += 1;
        i += 1;
    }
    at
}

// The `ducktape.tables` custom section of `$program`'s wasm module, from its `TABLES`: what
// `export!` and `export_awaiting!` expand to beside the exports.
#[doc(hidden)]
#[macro_export]
macro_rules! tables_section {
    ($program:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[used]
        #[unsafe(link_section = "ducktape.tables")]
        static DUCKTAPE_TABLES: [u8; $crate::tables::section_len(
            <$program as $crate::Program>::TABLES,
        )] = $crate::tables::section(<$program as $crate::Program>::TABLES);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLES: &[Table] = &[
        Table::shared("h/"),
        Table::owned("m/", "chat.message"),
        Table {
            prefix: "x/",
            held: Held::Owned(None),
        },
    ];

    #[test]
    fn the_section_is_the_borsh_of_the_declared_tables() {
        const LEN: usize = section_len(TABLES);
        let bytes: [u8; LEN] = section(TABLES);
        let declared = declared(TABLES);
        assert_eq!(declared.len(), 2, "an owned table of no kind is left out");
        assert_eq!(bytes.to_vec(), abi::encode(&declared));
        assert_eq!(section::<4>(&[]), [0; 4]);
    }

    #[test]
    fn the_section_is_named_as_the_kernel_reads_it() {
        let named = format!("link_section = \"{}\"", ownership::TABLES_SECTION);
        assert!(include_str!("tables.rs").contains(&named));
    }
}
