use super::*;

fn with_thinking(matter: &str) -> Result<Config, Malformed> {
    Config::parse(format!("+++\nmodel = \"sonnet\"\nmax_tokens = 1\n{matter}+++\nbody").as_bytes())
}

#[test]
fn thinking_is_optional_and_names_one_level() {
    assert_eq!(with_thinking("").map(|config| config.thinking), Ok(None));
    for level in Thinking::ALL {
        let matter = format!("thinking = \"{}\"\n", level.word());
        assert_eq!(
            with_thinking(&matter).map(|config| config.thinking),
            Ok(Some(*level))
        );
    }
    for matter in [
        "thinking = \"huge\"\n",
        "thinking = 3\n",
        "thinking = \"High\"\n",
    ] {
        assert!(
            matches!(with_thinking(matter), Err(Malformed::FrontMatter(_))),
            "{matter:?}"
        );
    }
}

#[test]
fn every_thinking_level_writes_and_parses_back() {
    let plain = Config {
        model: "sonnet".into(),
        max_tokens: 64,
        wait: Some(9),
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Think.".into(),
    };
    assert!(!plain.write().contains("thinking"));
    for level in Thinking::ALL {
        let config = Config {
            thinking: Some(*level),
            ..plain.clone()
        };
        assert_eq!(Config::parse(config.write().as_bytes()), Ok(config));
    }
}
