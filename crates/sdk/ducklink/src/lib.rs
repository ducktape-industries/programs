//! the `duck://` link: `duck://<label>-<salt>/<program>/<tail…>`, `<program>` a bare id or
//! `@<owner>/<name>`, read in one place by spelling alone (it reads no roster). The rules and
//! examples: README.md.
use abi::reason::INVALID_INPUT;

const SCHEME: &str = "duck://";

const SALT_HEX: std::ops::RangeInclusive<usize> = 8..=64;

/// the network, as the workspace registry keys it: `<label>#<salt>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainId {
    pub label: String,
    pub salt: Vec<u8>,
}

impl ChainId {
    /// the link spelling of the same pair: `<label>-<salt>`.
    pub fn authority(&self) -> String {
        format!("{}-{}", self.label, self.salt_hex())
    }

    /// the chain a network names: the network's name as the label, salted
    /// with the first four bytes (eight hex digits) of its genesis block's
    /// digest, which the node's status carries. `None` for a name that is
    /// no label, or a digest shorter than a salt.
    pub fn of(network: &str, genesis: &[u8]) -> Option<ChainId> {
        let salt = genesis.get(..4)?;
        let hex: String = salt.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("{network}#{hex}").parse().ok()
    }

    /// the salt's lowercase hex digits, two per byte.
    pub fn salt_hex(&self) -> String {
        self.salt.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// the registry's spelling.
impl std::fmt::Display for ChainId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}#{}", self.label, self.salt_hex())
    }
}

impl std::str::FromStr for ChainId {
    type Err = Refused;

    /// either spelling: `<label>#<salt>` as the registry writes it, or
    /// `<label>-<salt>` as a link writes it. Split from the RIGHT: a label may
    /// itself contain `-`, so only the LAST separator is the minted one.
    fn from_str(text: &str) -> Result<Self, Refused> {
        lowercase(text)?;
        let split = match text.contains('#') {
            true => text.rsplit_once('#'),
            false => text.rsplit_once('-'),
        };
        let Some((label, salt)) = split else {
            return Err(incomplete(text));
        };
        let labelled = !label.is_empty()
            && label
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'));
        let salted = SALT_HEX.contains(&salt.len())
            && salt.len() % 2 == 0
            && salt.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !labelled || !salted {
            return Err(incomplete(text));
        }
        let Ok(salt) = (0..salt.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&salt[at..at + 2], 16))
            .collect()
        else {
            return Err(incomplete(text));
        };
        Ok(ChainId {
            label: label.to_string(),
            salt,
        })
    }
}

/// a parsed `duck://` link. `tail` is everything after the program segment,
/// DECODED, and what it means is the program's question. [`Link::parse`] and
/// `Display` round-trip: a parsed link prints the string it came from, and
/// that string is the only one that parses to it.
///
/// The fields are public so a reader can take them apart; a struct literal
/// checks nothing. [`Link::parse`] and [`Link::new`] are the only ways to get
/// a value that round-trips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub chain: ChainId,
    pub program: String,
    pub tail: Vec<String>,
}

impl Link {
    pub fn parse(text: &str) -> Result<Self, Refused> {
        let rest = text.strip_prefix(SCHEME).ok_or_else(|| {
            Refused::new(
                INVALID_INPUT,
                format!("A ducktape link starts with `{SCHEME}`, and `{text}` does not."),
            )
        })?;
        if rest.contains('?') || rest.contains('#') {
            return Err(extra(text));
        }
        let Some((authority, path)) = rest.split_once('/') else {
            return Err(empty(text));
        };
        if authority.contains('@') || authority.contains(':') {
            return Err(extra(text));
        }
        if authority.is_empty() || path.is_empty() {
            return Err(empty(text));
        }
        let chain: ChainId = authority.parse()?;
        if path.split('/').any(str::is_empty) {
            return Err(empty(text));
        }
        let (program, rest) = read_program(path);
        program_segment(&program)?;
        Ok(Link {
            chain,
            program,
            tail: match rest {
                None => Vec::new(),
                Some(rest) => tail(rest)?,
            },
        })
    }

    /// a link from its parts, checked by the rules [`Link::parse`] applies —
    /// the constructor a program's own tail type prints itself through.
    pub fn new(chain: ChainId, program: &str, tail: Vec<String>) -> Result<Self, Refused> {
        chain.authority().parse::<ChainId>()?;
        program_segment(program)?;
        let link = Link {
            chain,
            program: program.to_string(),
            tail,
        };
        if link.tail.iter().any(String::is_empty) {
            return Err(empty(&link.to_string()));
        }
        for segment in &link.tail {
            validate_segment_name(segment, &encode(segment))?;
        }
        Ok(link)
    }
}

impl std::fmt::Display for Link {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{SCHEME}{}/{}",
            self.chain.authority(),
            written(&self.program)
        )?;
        for segment in &self.tail {
            write!(formatter, "/{}", encode(segment))?;
        }
        Ok(())
    }
}

/// `duck://<chain>/<program>/<tail…>` from a session's chain id text
/// (`<label>#<salt>`); `None` while the session names no chain, or when a
/// part breaks [`Link::new`]'s rules.
pub fn mint(chain: &str, program: &str, tail: &[&str]) -> Option<String> {
    let tail = tail.iter().map(|segment| (*segment).to_owned()).collect();
    Link::new(chain.parse().ok()?, program, tail)
        .ok()
        .map(|link| link.to_string())
}

pub fn path(program: &str, tail: &[&str]) -> Option<String> {
    program_segment(program).ok()?;
    let mut spelled = written(program);
    for segment in tail {
        let encoded = encode(segment);
        let named = !segment.is_empty() && validate_segment_name(segment, &encoded).is_ok();
        if !named {
            return None;
        }
        spelled.push('/');
        spelled.push_str(&encoded);
    }
    Some(spelled)
}

pub fn on_chain(chain: &str, path: &str) -> Option<String> {
    let (program, rest) = read_program(path);
    let segments = match rest {
        Some(rest) => tail(rest).ok()?,
        None => Vec::new(),
    };
    Link::new(chain.parse().ok()?, &program, segments)
        .ok()
        .map(|link| link.to_string())
}

/// a `/`-separated path of tail segments, each read from its one spelling
/// (`forge%3Aweb%3A3/42` → `["forge:web:3", "42"]`): the reading
/// [`Link::parse`] gives a link's tail, for a reader holding the path alone.
pub fn tail(path: &str) -> Result<Vec<String>, Refused> {
    path.split('/')
        .map(|segment| match segment.is_empty() {
            true => Err(Refused::new(
                INVALID_INPUT,
                format!("A duck:// path has no empty segment, and `{path}` does."),
            )),
            false => decode(segment),
        })
        .collect()
}

/// a tail segment that spells a number: decimal, no sign, no leading zero (`0`
/// itself aside), within `u64`. One spelling, so `7`, `07` and `+7` are not
/// three links to one thing; a program that numbers its tail reads it here and
/// words its own refusal.
pub fn number(segment: &str) -> Option<u64> {
    let canonical = !segment.is_empty()
        && segment.bytes().all(|byte| byte.is_ascii_digit())
        && (segment == "0" || !segment.starts_with('0'));
    match canonical {
        true => segment.parse().ok(),
        false => None,
    }
}

fn program_segment(program: &str) -> Result<(), Refused> {
    match program::is_id(program) {
        true => Ok(()),
        false => Err(Refused::new(
            INVALID_INPUT,
            format!(
                "A duck:// link names its program as a bare id or `@<owner>/<name>`, each name 1..=64 of [a-z0-9_-], and `{}` is not one.",
                written(program)
            ),
        )),
    }
}

fn written(program: &str) -> String {
    match program.contains('/') {
        true => format!("@{program}"),
        false => program.to_owned(),
    }
}

fn first_segment(path: &str) -> (&str, Option<&str>) {
    match path.split_once('/') {
        Some((first, rest)) => (first, Some(rest)),
        None => (path, None),
    }
}

fn read_program(path: &str) -> (String, Option<&str>) {
    let (first, rest) = first_segment(path);
    match (first.strip_prefix('@'), rest) {
        (Some(owner), Some(rest)) => {
            let (name, rest) = first_segment(rest);
            (format!("{owner}/{name}"), rest)
        }
        (Some(owner), None) => (format!("{owner}/"), None),
        (None, rest) => (first.to_owned(), rest),
    }
}

fn encode(name: &str) -> String {
    let mut spelled = String::with_capacity(name.len());
    for byte in name.bytes() {
        match rfc3986_unreserved(byte) {
            true => spelled.push(byte as char),
            false => spelled.push_str(&format!("%{byte:02X}")),
        }
    }
    spelled
}

fn rfc3986_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn decode(segment: &str) -> Result<String, Refused> {
    let refuse = |rule: &str| {
        Err(Refused::new(
            INVALID_INPUT,
            format!("A duck:// path segment {rule}, and `{segment}` does not."),
        ))
    };
    let mut decoded = Vec::with_capacity(segment.len());
    let mut bytes = segment.bytes();
    while let Some(byte) = bytes.next() {
        if rfc3986_unreserved(byte) {
            decoded.push(byte);
            continue;
        }
        if byte != b'%' {
            return refuse("writes only [A-Za-z0-9._~-] literally and every other byte as `%XX`");
        }
        let (high, low) = match (bytes.next(), bytes.next()) {
            (Some(high), Some(low)) if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() => {
                (high, low)
            }
            _ => return refuse("writes `%` only to open a `%XX` escape of two hex digits"),
        };
        if high.is_ascii_lowercase() || low.is_ascii_lowercase() {
            return refuse("writes a `%XX` escape in uppercase hex");
        }
        let nibble = |digit: u8| match digit.is_ascii_digit() {
            true => digit - b'0',
            false => digit - b'A' + 10,
        };
        let escaped = nibble(high) << 4 | nibble(low);
        if rfc3986_unreserved(escaped) {
            return refuse("writes [A-Za-z0-9._~-] literally, never as `%XX`");
        }
        decoded.push(escaped);
    }
    let Ok(decoded) = String::from_utf8(decoded) else {
        return refuse("decodes to UTF-8 text");
    };
    validate_segment_name(&decoded, segment)?;
    Ok(decoded)
}

fn validate_segment_name(name: &str, spelling: &str) -> Result<(), Refused> {
    let refuse = |rule: &str| {
        Err(Refused::new(
            INVALID_INPUT,
            format!("A duck:// path segment {rule}, and `{spelling}` does not."),
        ))
    };
    if name.contains(['/', '\0']) {
        return refuse("decodes to a name with no `/` and no NUL in it");
    }
    if name == "." || name == ".." {
        return refuse("names something other than `.` or `..`");
    }
    Ok(())
}

/// why a link was refused: a token to BRANCH on ([`abi::reason`]) and a
/// sentence to SHOW. Every rule a link breaks refuses as `INVALID_INPUT`,
/// because a caller fixes the link whatever rule it broke. `new` is public so
/// a program reading its own tail refuses in the same shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused {
    pub reason: &'static str,
    pub sentence: String,
}

impl Refused {
    pub fn new(reason: &'static str, sentence: impl Into<String>) -> Self {
        Self {
            reason,
            sentence: sentence.into(),
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.sentence)
    }
}

fn lowercase(text: &str) -> Result<(), Refused> {
    match text.bytes().any(|byte| byte.is_ascii_uppercase()) {
        true => Err(Refused::new(
            INVALID_INPUT,
            format!(
                "A duck:// chain id and program segment are lowercase and nothing case-folds them, but `{text}` carries an uppercase letter."
            ),
        )),
        false => Ok(()),
    }
}

fn incomplete(text: &str) -> Refused {
    Refused::new(
        INVALID_INPUT,
        format!(
            "A duck:// authority is the whole chain id `<label>-<salt>` (the registry's `<label>#<salt>`), with `<label>` matching [a-z0-9-] and `<salt>` an even count of {} to {} lowercase hex digits; `{text}` is not one.",
            SALT_HEX.start(),
            SALT_HEX.end(),
        ),
    )
}

fn extra(text: &str) -> Refused {
    Refused::new(
        INVALID_INPUT,
        format!(
            "A duck:// link carries no credentials, port, query or fragment — the node and its credential come from the workspace registry — but `{text}` carries one."
        ),
    )
}

fn empty(text: &str) -> Refused {
    Refused::new(
        INVALID_INPUT,
        format!(
            "A duck:// link is `duck://<label>-<salt>/<program>/…`, with an authority and at least the program segment, none of them empty; `{text}` leaves one empty."
        ),
    )
}

// The length of a bare link typed in prose (`run` up to the whitespace
// after it), less the sentence's punctuation trailing it: `.`, `,`, `;`,
// `:`, `!`, `?` and a quote mark, and a `)` that opens nowhere in the run
// (`(see https://x)`; `…/Foo_(bar)` keeps its own). A `.` after a `?`
// is the query's and stays.
pub fn bare_end(run: &str) -> usize {
    let mut end = run.len();
    while let Some(last) = run[..end].chars().next_back() {
        let before = &run[..end - last.len_utf8()];
        let trailing = match last {
            ',' | ';' | ':' | '!' | '?' | '\'' | '"' => true,
            '.' => !before.contains('?'),
            ')' => before.matches(')').count() >= before.matches('(').count(),
            _ => false,
        };
        if !trailing {
            break;
        }
        end = before.len();
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_link_leaves_the_sentences_punctuation_behind() {
        fn kept(run: &str) -> &str {
            &run[..bare_end(run)]
        }
        assert_eq!(
            kept("duck://n-b5b6ea90/files/shared/before-whole.png."),
            "duck://n-b5b6ea90/files/shared/before-whole.png"
        );
        assert_eq!(kept("https://x.example/a,"), "https://x.example/a");
        assert_eq!(kept("https://x.example/a)."), "https://x.example/a");
        assert_eq!(kept("https://x.example/a!?"), "https://x.example/a");
        assert_eq!(kept("https://x.example/a:"), "https://x.example/a");
        assert_eq!(
            kept("https://x.example/Foo_(bar)"),
            "https://x.example/Foo_(bar)"
        );
        assert_eq!(
            kept("https://x.example/s?q=a."),
            "https://x.example/s?q=a.",
            "a query's dot is the query's"
        );
        assert_eq!(kept("https://x.example/s?"), "https://x.example/s");
    }

    #[test]
    fn a_network_and_its_genesis_name_one_chain() {
        let chain = ChainId::of("testkit", &[0xb5, 0xb6, 0xea, 0x90, 0xff]).unwrap();
        assert_eq!(chain.to_string(), "testkit#b5b6ea90");
        assert_eq!(chain.authority(), "testkit-b5b6ea90");
        assert_eq!(ChainId::of("Test Kit", &[0; 32]), None);
        assert_eq!(ChainId::of("testkit", &[0; 3]), None);
    }

    fn chain() -> ChainId {
        "dognet-b5b6ea90".parse().expect("a chain id parses")
    }

    #[test]
    fn parses_and_round_trips_the_canonical_form() {
        let link = Link::parse("duck://dognet-b5b6ea90/chat/general/42").expect("parses");
        assert_eq!(link.chain, chain());
        assert_eq!(link.chain.salt, [0xb5, 0xb6, 0xea, 0x90]);
        assert_eq!(link.program, "chat");
        assert_eq!(link.tail, ["general", "42"]);
        for text in [
            "duck://dognet-b5b6ea90/forge/alice/my-crate",
            "duck://my-long-net-00000000/any_program/alice/my.crate_1",
            "duck://dognet-b5b6ea90/chat",
            "duck://dognet-b5b6ea90/files/%EB%B3%B4%EA%B3%A0%EC%84%9C%20Final.pdf",
        ] {
            let link = Link::parse(text).expect("parses");
            assert_eq!(link.to_string(), text);
            assert_eq!(Link::parse(&link.to_string()), Ok(link));
        }
    }

    #[test]
    fn any_program_name_reads_but_only_one_spelling_of_it() {
        assert!(Link::parse("duck://dognet-b5b6ea90/a-program_nobody_built/x").is_ok());
        let longest = format!("duck://dognet-b5b6ea90/{}/x", "p".repeat(64));
        assert!(Link::parse(&longest).is_ok());
        for text in [
            "duck://dognet-b5b6ea90/Chat/general",
            "duck://dognet-b5b6ea90/ch%61t/general",
            "duck://dognet-b5b6ea90/chat.v2/general",
            &format!("duck://dognet-b5b6ea90/{}/x", "p".repeat(65)),
        ] {
            assert!(Link::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn an_owned_program_is_written_after_an_at_and_a_bare_one_as_before() {
        let text = "duck://dognet-b5b6ea90/@ducktape-industries/chat/general/42";
        let link = Link::parse(text).expect("parses");
        assert_eq!(link.program, "ducktape-industries/chat");
        assert_eq!(link.tail, ["general", "42"]);
        assert_eq!(link.to_string(), text);
        let bare = Link::parse("duck://dognet-b5b6ea90/chat/general/42").expect("parses");
        assert_eq!(bare.program, "chat");
        assert_eq!(bare.tail, ["general", "42"]);
        let owned = Link::parse("duck://dognet-b5b6ea90/@o/forge").expect("parses");
        assert_eq!((owned.program.as_str(), owned.tail.len()), ("o/forge", 0));
        let minted = Link::new(chain(), "ducktape-industries/chat", vec!["general".into()]);
        assert_eq!(
            minted.unwrap().to_string(),
            "duck://dognet-b5b6ea90/@ducktape-industries/chat/general"
        );
        assert_eq!(
            path("o/forge", &["programs", "213"]).as_deref(),
            Some("@o/forge/programs/213")
        );
        let chain_text = "net#0a1b2c3d";
        assert_eq!(
            on_chain(chain_text, "@o/forge/programs/213"),
            mint(chain_text, "o/forge", &["programs", "213"])
        );
        for text in [
            "duck://dognet-b5b6ea90/@chat",
            "duck://dognet-b5b6ea90/@/chat",
            "duck://dognet-b5b6ea90/@Owner/chat",
            "duck://dognet-b5b6ea90/@o@p/chat",
            "duck://dognet-b5b6ea90/o@p/chat",
            "duck://dognet-b5b6ea90/@@o/chat",
        ] {
            let refused = Link::parse(text).expect_err(text).sentence;
            assert!(refused.contains("`@<owner>/<name>`"), "{text}: {refused}");
        }
        for program in ["@o/forge", "a/b/c", "o/", "/n"] {
            assert!(
                Link::new(chain(), program, Vec::new()).is_err(),
                "{program}"
            );
        }
        assert_eq!(on_chain(chain_text, "@o"), None);
        assert_eq!(on_chain(chain_text, "@o/"), None);
    }

    #[test]
    fn a_segment_carries_any_name_in_one_spelling() {
        let link = Link::new(
            chain(),
            "files",
            vec!["보고서 Final.pdf".into(), "A~b".into()],
        )
        .unwrap();
        assert_eq!(
            link.to_string(),
            "duck://dognet-b5b6ea90/files/%EB%B3%B4%EA%B3%A0%EC%84%9C%20Final.pdf/A~b"
        );
        assert_eq!(
            Link::parse(&link.to_string()).unwrap().tail[0],
            "보고서 Final.pdf"
        );
        for (text, rule) in [
            ("duck://dognet-b5b6ea90/files/a%20b/%2e%2e", "uppercase hex"),
            ("duck://dognet-b5b6ea90/files/%41", "never as `%XX`"),
            ("duck://dognet-b5b6ea90/files/a b", "literally"),
            ("duck://dognet-b5b6ea90/files/..", "`.` or `..`"),
            ("duck://dognet-b5b6ea90/files/a%2Fb", "no `/`"),
            ("duck://dognet-b5b6ea90/files//x", "none of them empty"),
            ("duck://dognet-b5b6ea90/files/x?y", "query or fragment"),
            ("duck://dognet-b5b6ea90/files/x#7", "query or fragment"),
            ("duck://user@dognet-b5b6ea90/files/x", "credentials"),
            ("duck://dognet/files/x", "whole chain id"),
            ("duck://b5b6ea90/files/x", "whole chain id"),
            ("https://dognet-b5b6ea90/files/x", "starts with"),
        ] {
            let refused = Link::parse(text).expect_err(text).sentence;
            assert!(refused.contains(rule), "{text}: {refused}");
        }
        assert!(Link::new(chain(), "files", vec!["a/b".into()]).is_err());
        assert!(Link::new(chain(), "files", vec![String::new()]).is_err());
    }

    #[test]
    fn a_chain_id_reads_and_prints_both_spellings() {
        let id: ChainId = "my-long-net#00ff00ff".parse().unwrap();
        assert_eq!(
            (id.label.as_str(), id.salt.as_slice()),
            ("my-long-net", &[0, 255, 0, 255][..])
        );
        assert_eq!(id.to_string(), "my-long-net#00ff00ff");
        assert_eq!(id.authority(), "my-long-net-00ff00ff");
        assert_eq!(id.authority().parse::<ChainId>().unwrap(), id);
        assert!("net-abcdef".parse::<ChainId>().is_err(), "salt under 8");
        assert!("net-abcdef012".parse::<ChainId>().is_err(), "odd salt");
        assert!(
            format!("net-{}", "ab".repeat(32))
                .parse::<ChainId>()
                .is_ok()
        );
        assert!(
            format!("net-{}", "ab".repeat(33))
                .parse::<ChainId>()
                .is_err()
        );
        assert!("Net-b5b6ea90".parse::<ChainId>().is_err());
    }

    #[test]
    fn a_tail_round_trips_any_name_and_refuses_a_broken_escape() {
        for name in ["forge:web:3", "보고서 #1", "a b", "dm-3-5", "~x.y_z"] {
            let link = mint("net#0a1b2c3d", "chat", &[name, "42"]).unwrap();
            let path = link.split_once("/chat/").unwrap().1;
            assert_eq!(tail(path).unwrap(), [name, "42"], "{link}");
            assert_eq!(tail(path).unwrap().join("/"), format!("{name}/42"));
        }
        assert_eq!(
            mint("net#0a1b2c3d", "chat", &["forge:web:3"]).unwrap(),
            "duck://net-0a1b2c3d/chat/forge%3Aweb%3A3"
        );
        for broken in ["%", "a%4", "%G0", "%zz", "%3a", "%FF", "%2F", "a//b", "%00"] {
            assert!(tail(broken).is_err(), "{broken}");
        }
    }

    #[test]
    fn a_number_has_one_spelling() {
        assert_eq!(number("0"), Some(0));
        assert_eq!(number("42"), Some(42));
        assert_eq!(number("18446744073709551615"), Some(u64::MAX));
        for bad in ["", "007", "+7", "-1", "seq", "18446744073709551616"] {
            assert_eq!(number(bad), None, "{bad}");
        }
    }

    #[test]
    fn mint_needs_a_chain_and_a_clean_tail() {
        let chain = ChainId::of("net", b"genesis").unwrap().to_string();
        let link = mint(&chain, "chat", &["design", "7"]).unwrap();
        assert_eq!(Link::parse(&link).unwrap().tail, ["design", "7"]);
        assert_eq!(mint("", "chat", &["design"]), None, "no chain yet");
        assert_eq!(mint(&chain, "chat", &[""]), None, "an empty segment");
    }

    #[test]
    fn a_path_takes_the_readers_chain_and_never_names_one() {
        let chain = "net#0a1b2c3d";
        let cases: [(&str, &[&str]); 3] = [
            ("forge", &["programs", "213"]),
            ("chat", &["forge:web:3", "보고서 #1"]),
            ("chat", &[]),
        ];
        for (program, tail) in cases {
            let spelled = path(program, tail).unwrap();
            assert_eq!(
                on_chain(chain, &spelled),
                mint(chain, program, tail),
                "{spelled}"
            );
        }
        assert_eq!(
            path("forge", &["programs", "213"]).as_deref(),
            Some("forge/programs/213")
        );
        assert_eq!(path("Forge", &["x"]), None);
        assert_eq!(path("forge", &[""]), None);
        assert_eq!(path("forge", &[".."]), None);
        assert_eq!(on_chain("", "forge/programs/213"), None, "no chain yet");
        for named in [
            "duck://other-00ff00ff/forge/programs/213",
            "https://x.example/a",
            "forge/",
            "forge//1",
            "/forge/1",
            "forge/%2F",
        ] {
            assert_eq!(on_chain(chain, named), None, "{named}");
        }
    }
}
