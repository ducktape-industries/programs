use super::*;

const PINNED: &str = "+++\nmodel = \"sonnet\"\n# the long one\nmax_tokens = 4096\nservice = \"ccs\"\nprovider = \"anthropic\"\n+++\n\nYou keep notes.\r\n\n  Indented, kept.\n";

fn pinned() -> Pin {
    Pin {
        service: Some("ccs".into()),
        provider: Some("anthropic".into()),
    }
}

#[test]
fn a_new_front_matter_keeps_the_pins_and_the_body_byte_for_byte() {
    let (config, pin) = Config::parse_pinned(PINNED.as_bytes()).unwrap();
    assert_eq!(pin, pinned());
    let thinks = Config {
        thinking: Some(Thinking::High),
        compact_at: Some(650_000),
        system: "not written: the body is the file's".into(),
        ..config
    };
    let written = with_front(PINNED.as_bytes(), &thinks, "anthropic").unwrap();
    let body = "\nYou keep notes.\r\n\n  Indented, kept.\n";
    let front = "model = \"sonnet\"\nmax_tokens = 4096\nthinking = \"high\"\ncompact_at = 650000\nservice = \"ccs\"\nprovider = \"anthropic\"\n";
    assert_eq!(written, format!("+++\n{front}+++\n{body}"));
    let (read, pin) = Config::parse_pinned(written.as_bytes()).unwrap();
    assert_eq!(pin, pinned());
    let system = "You keep notes.\r\n\n  Indented, kept.".to_owned();
    assert_eq!(read, Config { system, ..thinks });
}

#[test]
fn another_provider_drops_the_service_its_route_may_not_list() {
    let (config, _) = Config::parse_pinned(PINNED.as_bytes()).unwrap();
    let written = with_front(PINNED.as_bytes(), &config, "openai").unwrap();
    let (_, pin) = Config::parse_pinned(written.as_bytes()).unwrap();
    let openai = Pin {
        service: None,
        provider: Some("openai".into()),
    };
    assert_eq!(pin, openai);
    let unprovided = PINNED.replace("provider = \"anthropic\"\n", "");
    let written = with_front(unprovided.as_bytes(), &config, "anthropic").unwrap();
    let (_, pin) = Config::parse_pinned(written.as_bytes()).unwrap();
    let anthropic = Pin {
        service: None,
        provider: Some("anthropic".into()),
    };
    assert_eq!(
        pin, anthropic,
        "a file that named no provider keeps no pin either"
    );
}

#[test]
fn a_new_body_keeps_the_front_matter_byte_for_byte() {
    let written = with_body(PINNED.as_bytes(), "Answer about deploys.").unwrap();
    let close = PINNED.find("+++\n\n").unwrap();
    let front = &PINNED[..close];
    assert_eq!(written, format!("{front}+++\nAnswer about deploys.\n"));
    let (read, pin) = Config::parse_pinned(written.as_bytes()).unwrap();
    assert_eq!(pin, pinned());
    assert_eq!(read.system, "Answer about deploys.");
}

#[test]
fn a_front_matter_that_names_a_ccs_account_is_malformed() {
    let named = PINNED.replace(
        "service = \"ccs\"\n",
        "service = \"ccs\"\naccount = \"b\"\n",
    );
    let read = Config::parse_pinned(named.as_bytes());
    assert!(matches!(read, Err(Malformed::FrontMatter(_))), "{read:?}");
}

#[test]
fn a_file_with_no_front_matter_takes_neither() {
    let (config, _) = Config::parse_pinned(PINNED.as_bytes()).unwrap();
    let cases = [
        ("You keep notes.\n", Malformed::NoOpeningFence),
        ("+++\nmodel = \"sonnet\"\n", Malformed::NoClosingFence),
    ];
    for (file, malformed) in cases {
        assert_eq!(with_body(file.as_bytes(), "x"), Err(malformed.clone()));
        assert_eq!(
            with_front(file.as_bytes(), &config, "anthropic"),
            Err(malformed)
        );
    }
}
