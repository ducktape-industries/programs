//! What a program tells an agent about its ops and queries: a line on what it is for, and for each
//! variant what it does, with an example to edit. `export!` writes it as the `ducktape.tools`
//! custom section, JSON text that any harness reads without the program's schema.

pub const SECTION: &str = "ducktape.tools";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Op,
    Query,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    pub kind: Kind,
    pub variant: &'static str,
    pub says: &'static str,
    pub example: Option<&'static str>,
}

pub const fn op(variant: &'static str, says: &'static str) -> Tool {
    Tool {
        kind: Kind::Op,
        variant,
        says,
        example: None,
    }
}

pub const fn query(variant: &'static str, says: &'static str) -> Tool {
    Tool {
        kind: Kind::Query,
        variant,
        says,
        example: None,
    }
}

impl Tool {
    // A value of the variant as JSON text, which `borsh_json::tools::check` holds to the schema.
    pub const fn example(self, example: &'static str) -> Tool {
        Tool {
            example: Some(example),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tools {
    pub about: &'static str,
    pub tools: &'static [Tool],
}

impl Tools {
    pub const NONE: Tools = Tools {
        about: "",
        tools: &[],
    };

    pub const fn new(about: &'static str, tools: &'static [Tool]) -> Tools {
        Tools { about, tools }
    }

    pub const fn is_empty(&self) -> bool {
        self.about.is_empty() && self.tools.is_empty()
    }
}

// The section's bytes: `{"about": …, "ops": {variant: {"says": …, "example": …}}, "queries": …}`,
// each example as its own JSON text, and `{}` for a program that says nothing.
pub fn json(tools: &Tools) -> Vec<u8> {
    let mut out = vec![0; section_len(tools)];
    emit(tools, &mut out, true);
    out
}

pub const fn section_len(tools: &Tools) -> usize {
    let mut none: [u8; 0] = [];
    emit(tools, &mut none, false)
}

pub const fn section<const N: usize>(tools: &Tools) -> [u8; N] {
    let mut out = [0u8; N];
    let at = emit(tools, &mut out, true);
    assert!(at == N, "the section is not section_len's length");
    out
}

const fn emit(tools: &Tools, out: &mut [u8], write: bool) -> usize {
    let mut at = put(out, write, 0, b"{");
    if !tools.is_empty() {
        at = put(out, write, at, b"\"about\":");
        at = text(out, write, at, tools.about.as_bytes());
        at = put(out, write, at, b",\"ops\":{");
        at = group(tools, Kind::Op, out, write, at);
        at = put(out, write, at, b",\"queries\":{");
        at = group(tools, Kind::Query, out, write, at);
    }
    put(out, write, at, b"}")
}

const fn group(tools: &Tools, kind: Kind, out: &mut [u8], write: bool, at: usize) -> usize {
    let mut at = at;
    let mut first = true;
    let mut i = 0;
    while i < tools.tools.len() {
        let tool = &tools.tools[i];
        i += 1;
        if tool.kind as u8 != kind as u8 {
            continue;
        }
        if !first {
            at = put(out, write, at, b",");
        }
        first = false;
        at = text(out, write, at, tool.variant.as_bytes());
        at = put(out, write, at, b":{\"says\":");
        at = text(out, write, at, tool.says.as_bytes());
        if let Some(example) = tool.example {
            at = put(out, write, at, b",\"example\":");
            at = put(out, write, at, example.as_bytes());
        }
        at = put(out, write, at, b"}");
    }
    put(out, write, at, b"}")
}

const fn put(out: &mut [u8], write: bool, at: usize, bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        if write {
            out[at + i] = bytes[i];
        }
        i += 1;
    }
    at + bytes.len()
}

const HEX: &[u8; 16] = b"0123456789abcdef";

const fn text(out: &mut [u8], write: bool, at: usize, text: &[u8]) -> usize {
    let mut at = put(out, write, at, b"\"");
    let mut i = 0;
    while i < text.len() {
        let byte = text[i];
        i += 1;
        at = match byte {
            b'"' => put(out, write, at, b"\\\""),
            b'\\' => put(out, write, at, b"\\\\"),
            0..0x20 => {
                let escaped = [
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[(byte >> 4) as usize],
                    HEX[(byte & 15) as usize],
                ];
                put(out, write, at, &escaped)
            }
            _ => put(out, write, at, &[byte]),
        };
    }
    put(out, write, at, b"\"")
}

// The `ducktape.tools` custom section of `$program`'s wasm module, from its `TOOLS`: what
// `export!` and `export_awaiting!` expand to beside the exports.
#[doc(hidden)]
#[macro_export]
macro_rules! tools_section {
    ($program:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[used]
        #[unsafe(link_section = "ducktape.tools")]
        static DUCKTAPE_TOOLS: [u8; $crate::tools::section_len(
            &<$program as $crate::Program>::TOOLS,
        )] = $crate::tools::section(&<$program as $crate::Program>::TOOLS);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOLS: Tools = Tools::new(
        "Polls \"open\" and closed.",
        &[
            op("Open", "Open a poll.\nIts id is yours.").example(r#"{"Open":{"id":"p"}}"#),
            query("Poll", "One poll by id."),
            op("Close", "Close a poll\\all of it."),
        ],
    );

    #[test]
    fn the_section_is_json_text_of_ops_and_queries_with_each_example_as_written() {
        let text = String::from_utf8(json(&TOOLS)).unwrap();
        assert_eq!(
            text,
            concat!(
                r#"{"about":"Polls \"open\" and closed.","#,
                r#""ops":{"Open":{"says":"Open a poll.\u000aIts id is yours.","example":{"Open":{"id":"p"}}},"#,
                r#""Close":{"says":"Close a poll\\all of it."}},"#,
                r#""queries":{"Poll":{"says":"One poll by id."}}}"#,
            )
        );
        const LEN: usize = section_len(&TOOLS);
        let bytes: [u8; LEN] = section(&TOOLS);
        assert_eq!(bytes.to_vec(), json(&TOOLS));
    }

    #[test]
    fn a_program_that_says_nothing_has_an_empty_object() {
        assert_eq!(json(&Tools::NONE), b"{}");
        assert!(Tools::NONE.is_empty() && !TOOLS.is_empty());
    }

    #[test]
    fn the_section_is_named_as_readers_read_it() {
        let named = format!("link_section = \"{SECTION}\"");
        assert!(include_str!("tools.rs").contains(&named));
    }
}
