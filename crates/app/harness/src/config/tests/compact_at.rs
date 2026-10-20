use super::*;

const FRONT: &str = "+++\nmodel = \"opus\"\nmax_tokens = 8\n";

fn parsed(more: &str) -> Result<Config, Malformed> {
    Config::parse(format!("{FRONT}{more}+++\nPrompt.\n").as_bytes())
}

#[test]
fn compact_at_parses_and_writes_back() {
    let config = parsed("compact_at = 650000\n").unwrap();
    assert_eq!(config.compact_at, Some(650_000));
    assert!(
        config
            .write(&Pin::default())
            .contains("\ncompact_at = 650000\n")
    );
    assert_eq!(
        Config::parse(config.write(&Pin::default()).as_bytes()),
        Ok(config.clone())
    );
    assert_eq!(parsed("").map(|config| config.compact_at), Ok(None));
    assert!(
        !parsed("")
            .unwrap()
            .write(&Pin::default())
            .contains("compact_at")
    );
}

#[test]
fn compact_at_is_at_least_one_token() {
    assert_eq!(parsed("compact_at = 0\n"), Err(Malformed::NoCompactAt));
    assert_eq!(
        Malformed::NoCompactAt.to_string(),
        "compact_at, when given, is at least 1 token"
    );
}

#[test]
fn an_unknown_key_beside_it_is_still_refused() {
    let typo = parsed("compact_after = 650000\n");
    assert!(matches!(typo, Err(Malformed::FrontMatter(_))), "{typo:?}");
}
