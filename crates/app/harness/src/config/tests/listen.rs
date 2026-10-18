use super::*;

fn with(matter: &str) -> Result<Config, Malformed> {
    Config::parse(format!("+++\nmodel = \"sonnet\"\nmax_tokens = 1\n{matter}+++\nbody").as_bytes())
}

#[test]
fn listen_names_channels_and_absent_names_none() {
    assert_eq!(with("").map(|config| config.listen), Ok(Vec::new()));
    let both = "listen = [\"engineering\", \"dm-1-2\"]\n";
    assert_eq!(
        with(both).map(|config| config.listen),
        Ok(vec!["engineering".to_owned(), "dm-1-2".to_owned()])
    );
    for matter in ["listen = \"engineering\"\n", "listen = [1]\n"] {
        assert!(
            matches!(with(matter), Err(Malformed::FrontMatter(_))),
            "{matter:?}"
        );
    }
}

#[test]
fn settle_is_a_duration_in_ms_s_or_m_and_absent_is_off() {
    assert_eq!(with("").map(|config| config.settle), Ok(None));
    for (given, millis) in [
        ("10s", 10_000),
        ("250ms", 250),
        ("2m", 120_000),
        (" 3 s", 3_000),
    ] {
        let matter = format!("settle = \"{given}\"\n");
        assert_eq!(
            with(&matter).map(|config| config.settle),
            Ok(Some(millis)),
            "{given:?}"
        );
    }
    for given in ["0s", "10", "ten s", "-1s", "1h", ""] {
        let matter = format!("settle = \"{given}\"\n");
        assert_eq!(
            with(&matter),
            Err(Malformed::NoSettle(given.to_owned())),
            "{given:?}"
        );
    }
    assert!(matches!(
        with("settle = 10\n"),
        Err(Malformed::FrontMatter(_))
    ));
}

#[test]
fn listen_and_settle_write_and_parse_back() {
    let plain = Config {
        model: "sonnet".into(),
        max_tokens: 64,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Hear.".into(),
    };
    let written = plain.write();
    assert!(!written.contains("listen") && !written.contains("settle"));
    for settle in [10_000, 250, 120_000, 1_500] {
        let config = Config {
            listen: vec!["engineering".into(), "a \"quoted\" id".into()],
            settle: Some(settle),
            ..plain.clone()
        };
        assert_eq!(Config::parse(config.write().as_bytes()), Ok(config));
    }
}
