//! Static metadata for a ducktape view. Parsing never instantiates or runs a view.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::methods::Capability;

pub const MANIFEST_SECTION: &str = "ducktape.view.manifest";

/// The manifest's lines, in order: what `export_view!` writes and
/// [`Manifest::parse`] reads, listed in `tests/golden/schema.txt` so a
/// line added or moved moves [`crate::WIRE_ID`] like any other shape.
pub const LINES: [&str; 10] = [
    "ducktape.view.manifest",
    "name",
    "description",
    "capabilities",
    "min_width",
    "wire_id",
    "targets",
    "icon",
    "tint",
    "width",
];

/// Crosses the wire as `blob.manifest`'s answer, so its shape is in
/// `tests/golden/schema.txt` with the rest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Manifest {
    /// the [`crate::WIRE_ID`] the view was built against
    // since J-085 the [`crate::WIRE_LEVEL`] it was built at, in decimal
    // ([`Manifest::level`])
    pub wire_id: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    pub min_width: u32,
    // Line 7, read past and meaning nothing: a view addresses any
    // program. Views built before J-054 listed programs there; one
    // built since writes it empty.
    pub targets: Vec<String>,
    pub icon: String,
    pub tint: Tint,
    /// The width, in logical px, a desk window holding the view opens at;
    /// none for the app's own share of the desk.
    pub width: Option<u32>,
}

/// The colour a view asks its tile in, where the app lists it (a phone's
/// Home): a name from this closed set, never a value. The host owns what
/// each name looks like, in the light and in the dark, so a view picks a
/// meaning and every view's tile stays legible beside the others. `None`
/// is the neutral tile, and what a host reads for a tint it cannot name
/// ([`Manifest::parse`]).
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Tint {
    #[default]
    None,
    Blue,
    Teal,
    Green,
    Lime,
    Amber,
    Orange,
    Red,
    Pink,
    Violet,
}

impl Tint {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Blue,
        Self::Teal,
        Self::Green,
        Self::Lime,
        Self::Amber,
        Self::Orange,
        Self::Red,
        Self::Pink,
        Self::Violet,
    ];

    /// The name a manifest spells it with.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Lime => "lime",
            Self::Amber => "amber",
            Self::Orange => "orange",
            Self::Red => "red",
            Self::Pink => "pink",
            Self::Violet => "violet",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == name)
    }
}

pub const fn is_icon(icon: &str) -> bool {
    icon.is_empty() || (crate::safe_asset_path(icon) && !has_control(icon))
}

/// A control character (`char::is_control`) somewhere in `text`.
const fn has_control(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // U+0080..=U+009F is `c2 80`..=`c2 9f`
        let c1 = bytes[i] == 0xc2 && i + 1 < bytes.len() && matches!(bytes[i + 1], 0x80..=0x9f);
        if bytes[i] < 0x20 || bytes[i] == 0x7f || c1 {
            return true;
        }
        i += 1;
    }
    false
}

/// Extracts exactly one current manifest from a view's core module.
/// Returns `None` for missing, duplicate, malformed, or out-of-bounds metadata.
/// This parses the binary structure; it does not validate the exports or execute it.
#[cfg(feature = "manifest")]
pub fn read_manifest(bytes: &[u8]) -> Option<Manifest> {
    read_manifest_noting(bytes).map(|(manifest, _)| manifest)
}

// [`read_manifest`], and the capabilities it left out
// ([`Manifest::parse_noting`]).
#[cfg(feature = "manifest")]
pub fn read_manifest_noting(bytes: &[u8]) -> Option<(Manifest, Vec<String>)> {
    let mut payloads = wasmparser::Parser::new(0).parse_all(bytes);
    // A view is a core module; a component is not something the host
    // instantiates, so it is not in the catalog.
    let Some(Ok(wasmparser::Payload::Version {
        encoding: wasmparser::Encoding::Module,
        ..
    })) = payloads.next()
    else {
        return None;
    };
    let mut manifest = None;
    for payload in payloads {
        if let wasmparser::Payload::CustomSection(section) = payload.ok()?
            && section.name() == MANIFEST_SECTION
        {
            if manifest.is_some() {
                return None;
            }
            manifest = Some(Manifest::parse_noting(
                std::str::from_utf8(section.data()).ok()?,
            )?);
        }
    }
    manifest
}

impl Manifest {
    // Parses the ten-line ([`LINES`]) `ducktape.view.manifest` text and
    // its bounds; an icon that is not an asset path ([`is_icon`]) refuses
    // it. Line 7 is read as a list and given no meaning. Since J-085 a
    // line past the ones this host knows is read past, and a capability
    // it does not know is left out: it grants nothing on a host that has
    // no such capability, and [`Manifest::parse_noting`] says which were
    // left out, for the host to log.
    //
    // The tint and the width alone are read leniently: a tint this host
    // does not know, a width that is not one in canonical decimal, or no
    // ninth or tenth line at all (a view built before them), reads as
    // [`Tint::None`], the neutral tile, and as no width, the app's own
    // share. A look grants nothing, so reading it as none narrows
    // nothing, and a view built against another wire still runs (a host
    // refuses a manifest, never a look).
    pub fn parse(text: &str) -> Option<Self> {
        Self::parse_noting(text).map(|(manifest, _)| manifest)
    }

    // [`Manifest::parse`], and the capabilities line 4 names that this
    // host does not know, which the manifest leaves out.
    pub fn parse_noting(text: &str) -> Option<(Self, Vec<String>)> {
        if text.chars().any(|c| c.is_control() && c != '\n') {
            return None;
        }
        let mut lines = text.split('\n');
        if lines.next()? != "ducktape.view.manifest" {
            return None;
        }
        let name = lines.next()?.to_owned();
        let description = lines.next()?.to_owned();
        let mut unknown = Vec::new();
        let mut capabilities = Vec::new();
        for name in list(lines.next()?)? {
            match Capability::parse(name) {
                Some(known) => capabilities.push(known),
                None if name.is_empty() => return None,
                None => unknown.push(name.to_owned()),
            }
        }
        let min_width = px(lines.next()?)?;
        let wire_id = lines.next()?.to_owned();
        let targets: Vec<String> = list(lines.next()?)?
            .into_iter()
            .map(str::to_owned)
            .collect();
        let icon = lines.next()?.to_owned();
        let tint = lines.next().and_then(Tint::parse).unwrap_or_default();
        let width = lines.next().and_then(px);
        let manifest = Self {
            wire_id,
            name,
            description,
            capabilities,
            min_width,
            targets,
            icon,
            tint,
            width,
        };
        manifest.within_bounds().then_some((manifest, unknown))
    }

    // The wire level the view was built at ([`crate::WIRE_LEVEL`]): line 6
    // in canonical decimal. A view built before J-085 wrote its `WIRE_ID`
    // there, sixteen hex digits, which is level 0.
    pub fn level(&self) -> u32 {
        self.wire_id
            .parse::<u32>()
            .ok()
            .filter(|level| level.to_string() == self.wire_id)
            .unwrap_or(0)
    }

    fn within_bounds(&self) -> bool {
        !self.name.is_empty()
            && !self.wire_id.is_empty()
            && self
                .wire_id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            && is_icon(&self.icon)
    }
}

/// A width line: logical px in canonical decimal (no sign, no leading
/// zero, no spaces), at least 1.
fn px(line: &str) -> Option<u32> {
    line.parse::<u32>()
        .ok()
        .filter(|n| n.to_string() == line && *n >= 1)
}

/// A comma-terminated list line (`a,b,`): its items, none for an empty
/// line; `None` for a line that is not one.
fn list(line: &str) -> Option<Vec<&str>> {
    match line.is_empty() {
        true => Some(Vec::new()),
        false => Some(line.strip_suffix(',')?.split(',').collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "manifest")]
    #[test]
    fn extraction_rejects_duplicate_and_truncated_sections() {
        let mut bytes = b"\0asm\x01\0\0\0".to_vec();
        let text = b"ducktape.view.manifest\nSized\n\n\n640\n0123abcd\n\n\nnone\n";
        let mut section = vec![
            0,
            (1 + MANIFEST_SECTION.len() + text.len()) as u8,
            MANIFEST_SECTION.len() as u8,
        ];
        section.extend_from_slice(MANIFEST_SECTION.as_bytes());
        section.extend_from_slice(text);
        bytes.extend_from_slice(&section);
        assert_eq!(read_manifest(&bytes).unwrap().min_width, 640);
        let mut duplicate = bytes.clone();
        duplicate.extend_from_slice(&section);
        assert!(
            read_manifest(&duplicate).is_none(),
            "duplicate manifest accepted"
        );
        bytes.extend_from_slice(&[0, 127]);
        assert!(
            read_manifest(&bytes).is_none(),
            "truncated section accepted"
        );
        assert!(
            read_manifest(b"\0asm\x0d\0\x01\0").is_none(),
            "component accepted"
        );
    }

    // Claim: untrusted module metadata has one strict format. Dropping
    // those guards is Red.
    #[test]
    fn the_manifest_format_is_strict() {
        let good =
            "ducktape.view.manifest\nSized\nDescription\nclock,store,\n480\n0123abcd\n\n\nnone\n";
        let parsed = Manifest::parse(good).unwrap();
        assert_eq!(parsed.capabilities, [Capability::Clock, Capability::Store]);
        assert_eq!(
            (&*parsed.name, &*parsed.description),
            ("Sized", "Description")
        );
        assert!(parsed.targets.is_empty());
        assert!(parsed.icon.is_empty());
        assert_eq!(parsed.tint, Tint::None);
        assert_eq!(parsed.width, None);
        for invalid in [
            "Sized\nDescription\nclock,", // no header
            "ducktape.view\nSized\nDescription\nclock,\n480\n0123abcd\n\n\nnone\n", // another header
            "ducktape.view.manifest\nSized\nDescription\n\n480",
            "ducktape.view.manifest\nSized\nDescription\n\n480\n0123abcd", // six lines: the shape before targets
            "ducktape.view.manifest\nSized\nDescription\n\n480\n0123abcd\n", // seven lines: the shape before icon
            "ducktape.view.manifest\nSized\nDescription\nclock\n480\n0123abcd\n\n\nnone\n",
            "ducktape.view.manifest\nSized\nDescription\nclock,,\n480\n0123abcd\n\n\nnone\n",
        ] {
            assert!(
                Manifest::parse(invalid).is_none(),
                "accepted malformed manifest: {invalid}"
            );
        }
    }

    #[test]
    fn a_manifest_carries_its_min_width() {
        let parse = |width: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n{width}\n0123abcd\n\n\nnone\n"
            ))
        };
        assert_eq!(parse("480").unwrap().min_width, 480);
        assert_eq!(parse("1").unwrap().min_width, 1);
        assert_eq!(parse("8192").unwrap().min_width, 8192);
        for invalid in [
            "",
            "0",
            "01",
            "+480",
            "-1",
            "480.0",
            " 480",
            "4294967296",
            "none",
            "1180,760",
        ] {
            assert!(parse(invalid).is_none(), "accepted {invalid:?}");
        }
        // a line past the tenth is a later build's, read past
        assert!(
            Manifest::parse("ducktape.view.manifest\nApp\n\n\n480\n480\n\n\nnone\n\n").is_some(),
            "an eleventh line refused"
        );
    }

    // Claim: line 7 is read past. A view built when it listed the programs
    // the view addressed still loads, whatever it listed, and one that can
    // address a program (`op`, `program`) loads with the line empty: a view
    // addresses any program, and its manifest has no say in which.
    #[test]
    fn line_seven_is_read_and_grants_nothing() {
        let parse = |caps: &str, targets: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n{caps}\n480\n0123abcd\n{targets}\n\nnone\n"
            ))
        };
        assert_eq!(
            parse("op,program,", "chat,identity,").unwrap().targets,
            ["chat", "identity"]
        );
        for caps in ["op,", "program,", "op,program,", ""] {
            assert!(parse(caps, "").unwrap().targets.is_empty(), "{caps:?}");
        }
        assert!(
            parse("op,", "a/b,").is_some(),
            "line 7 names nothing to check"
        );
        assert!(parse("op,", "chat").is_none(), "a list ends in a comma");
    }

    #[test]
    fn a_manifest_carries_its_icon() {
        let parse = |icon: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n480\n0123abcd\n\n{icon}\nnone\n"
            ))
        };
        assert_eq!(parse("icons/hammer.svg").unwrap().icon, "icons/hammer.svg");
        assert_eq!(parse("").unwrap().icon, "");
        let long = format!("icons/{}", "x".repeat(1000));
        assert_eq!(parse(&long).unwrap().icon, long);
        for invalid in ["/abs", "a/../b", "a/./b", "..", "a//b", "a/", "a\\b", "c:x"] {
            assert!(parse(invalid).is_none(), "accepted {invalid:?}");
        }
        // a view's build asks the same rule, before there is a text to parse
        for control in ["a\nb", "a\tb", "a\u{7f}b", "a\u{85}b"] {
            assert!(!is_icon(control), "accepted {control:?}");
        }
        assert!(is_icon("icons/\u{e9}.svg"));
    }

    // Claim: line 9 is a tint by its name, one of the closed set. Unlike a
    // capability, a tint this host cannot name (a value, a name from a
    // later set, or no line: a view built before tints) reads as none, the
    // neutral tile, and the rest of the manifest stands.
    #[test]
    fn a_manifest_carries_its_tint() {
        let parse = |tint: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n480\n0123abcd\n\n\n{tint}\n"
            ))
        };
        for tint in Tint::ALL {
            assert_eq!(parse(tint.as_str()).unwrap().tint, tint);
        }
        for unknown in ["", "Blue", " blue", "blue,", "#2a6", "grey", "indigo"] {
            assert_eq!(parse(unknown).unwrap().tint, Tint::None, "{unknown:?}");
        }
        let before =
            Manifest::parse("ducktape.view.manifest\nApp\n\nclock,\n480\n0123abcd\n\nicons/a.svg")
                .expect("an eight-line manifest reads");
        assert_eq!(
            (before.tint, &*before.icon, &*before.capabilities),
            (Tint::None, "icons/a.svg", &[Capability::Clock][..])
        );
    }

    // Claim: line 10 is the width a desk window opens at, in line 5's
    // canonical decimal, or empty for none. Like the tint it grants
    // nothing: a width this host cannot read, or no line at all (a view
    // built before widths), reads as none, the app's own share, and the
    // rest of the manifest stands.
    #[test]
    fn a_manifest_carries_its_width() {
        let parse = |width: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n480\n0123abcd\n\n\nblue\n{width}"
            ))
        };
        assert_eq!(parse("1200").unwrap().width, Some(1200));
        assert_eq!(parse("1").unwrap().width, Some(1));
        assert_eq!(parse("").unwrap().width, None);
        for unreadable in [
            "0",
            "01",
            "+1200",
            "-1",
            "1200.0",
            " 1200",
            "4294967296",
            "none",
            "1200,800",
        ] {
            let manifest = parse(unreadable).unwrap();
            assert_eq!(
                (manifest.width, manifest.tint),
                (None, Tint::Blue),
                "{unreadable:?}"
            );
        }
        let before = Manifest::parse(
            "ducktape.view.manifest\nApp\n\nclock,\n480\n0123abcd\n\nicons/a.svg\nblue",
        )
        .expect("a nine-line manifest reads");
        assert_eq!(
            (before.width, before.tint, &*before.icon),
            (None, Tint::Blue, "icons/a.svg")
        );
    }

    // Claim (J-085): a manifest a later build wrote still reads. A
    // capability this host does not know is left out and named, the lines
    // after the tenth are read past, and the level is line 6 in decimal,
    // or 0 for a view built before levels.
    #[test]
    fn a_later_builds_manifest_reads() {
        let text = "ducktape.view.manifest\nApp\n\nclock,teleport,store,\n480\n7\n\n\nblue\n900\nan eleventh line\n";
        let (manifest, unknown) = Manifest::parse_noting(text).unwrap();
        assert_eq!(
            manifest.capabilities,
            [Capability::Clock, Capability::Store]
        );
        assert_eq!(unknown, ["teleport"]);
        assert_eq!((manifest.width, manifest.tint), (Some(900), Tint::Blue));
        assert_eq!(manifest.level(), 7);
        let level = |id: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n480\n{id}\n\n\nnone\n"
            ))
            .unwrap()
            .level()
        };
        assert_eq!(level("1"), 1);
        assert_eq!(level(&crate::WIRE_LEVEL.to_string()), crate::WIRE_LEVEL);
        for before in ["cd5911c62028048f", "ce6c54ef404b8852", "01", "0123abcd"] {
            assert_eq!(level(before), 0, "{before}");
        }
    }

    #[test]
    fn the_wire_id_is_lowercase_hex() {
        let parse = |id: &str| {
            Manifest::parse(&format!(
                "ducktape.view.manifest\nApp\n\n\n480\n{id}\n\n\nnone\n"
            ))
        };
        assert_eq!(
            parse("0123456789abcdef").unwrap().wire_id,
            "0123456789abcdef"
        );
        assert!(parse(crate::WIRE_ID).is_some());
        for invalid in ["", "ABCDEF", "xyz", " 0a", "0a "] {
            assert!(parse(invalid).is_none(), "accepted {invalid:?}");
        }
    }
}
