mod compact_at;
mod compaction_instruction;
mod listen;
mod server_tools;
mod splice;
mod thinking;

use super::*;

const SCOUT: &str =
    "+++\nmodel = \"sonnet\"\nmax_tokens = 4096\nwait = 600\n+++\n\nYou keep notes in /shared.\n";

#[test]
fn a_file_parses_into_its_front_matter_and_its_system_prompt() {
    assert_eq!(
        Config::parse(SCOUT.as_bytes()),
        Ok(Config {
            model: "sonnet".into(),
            max_tokens: 4096,
            wait: Some(600),
            thinking: None,
            server_tools: Vec::new(),
            listen: Vec::new(),
            settle: None,
            compact_at: None,
            system: "You keep notes in /shared.".into(),
        })
    );
    let crlf = SCOUT.replace('\n', "\r\n");
    assert_eq!(
        Config::parse(crlf.as_bytes()).map(|config| config.max_tokens),
        Ok(4096)
    );
    let no_wait = "+++\nmodel = \"sonnet\"\nmax_tokens = 1\n+++";
    assert_eq!(
        Config::parse(no_wait.as_bytes()),
        Ok(Config {
            model: "sonnet".into(),
            max_tokens: 1,
            wait: None,
            thinking: None,
            server_tools: Vec::new(),
            listen: Vec::new(),
            settle: None,
            compact_at: None,
            system: String::new(),
        })
    );
}

#[test]
fn each_paragraph_of_the_system_prompt_is_one_block_in_order() {
    let config = Config {
        model: "sonnet".into(),
        max_tokens: 1,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Who you are.\n\n\nWhat you do,\nline two.\n  \n- a list\r\n- of two".into(),
    };
    assert_eq!(
        config.paragraphs(),
        [
            "Who you are.",
            "What you do,\nline two.",
            "- a list\n- of two"
        ]
    );
    let empty = Config {
        system: String::new(),
        ..config
    };
    assert!(empty.paragraphs().is_empty());
}

#[test]
fn what_it_writes_parses_back_to_itself() {
    let config = Config {
        model: "a \"quoted\" id".into(),
        max_tokens: 8192,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Line one.\n\n+++ is not a fence inside a line.".into(),
    };
    assert_eq!(
        Config::parse(config.write(&Pin::default()).as_bytes()),
        Ok(config.clone())
    );
    let waits = Config {
        wait: Some(30),
        ..config
    };
    assert_eq!(
        Config::parse(waits.write(&Pin::default()).as_bytes()),
        Ok(waits)
    );
}

#[test]
fn a_malformed_file_says_what_is_wrong() {
    let front = |matter: &str| format!("+++\n{matter}+++\nbody");
    let cases = [
        (vec![0xff, 0xfe], Malformed::NotText),
        (b"model = \"sonnet\"\n".to_vec(), Malformed::NoOpeningFence),
        (
            b"+++\nmodel = \"sonnet\"\nmax_tokens = 1\n".to_vec(),
            Malformed::NoClosingFence,
        ),
        (
            front("model = \"sonnet\"\nmax_tokens = 0\n").into_bytes(),
            Malformed::NoTokens,
        ),
        (
            front("model = \"sonnet\"\nmax_tokens = 1\nwait = 0\n").into_bytes(),
            Malformed::NoWait,
        ),
    ];
    for (bytes, malformed) in cases {
        assert_eq!(Config::parse(&bytes), Err(malformed));
    }
    for matter in [
        "model = \"sonnet\"\n",
        "max_tokens = 1\n",
        "model = \"sonnet\"\nmax_tokens = 1\nwiat = 3\n",
        "model = sonnet\nmax_tokens = 1\n",
        "model = \"sonnet\"\nmax_tokens = -1\n",
    ] {
        let parsed = Config::parse(front(matter).as_bytes());
        assert!(
            matches!(parsed, Err(Malformed::FrontMatter(_))),
            "{matter:?}: {parsed:?}"
        );
    }
}
