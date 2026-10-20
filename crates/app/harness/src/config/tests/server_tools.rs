use super::*;

fn tools_of(matter: &str) -> Result<Vec<ServerTool>, Malformed> {
    let text = format!("+++\nmodel = \"sonnet\"\nmax_tokens = 1\n{matter}+++\nbody");
    Config::parse(text.as_bytes()).map(|config| config.server_tools)
}

#[test]
fn server_tools_are_optional_and_listed_by_name() {
    assert_eq!(tools_of(""), Ok(Vec::new()));
    assert_eq!(tools_of("server_tools = []\n"), Ok(Vec::new()));
    assert_eq!(
        tools_of("server_tools = [\"web_search\", \"web_fetch\"]\n"),
        Ok(vec![ServerTool::WebSearch, ServerTool::WebFetch])
    );
    assert_eq!(
        tools_of("server_tools = [\"web_fetch\", \"web_fetch\"]\n"),
        Ok(vec![ServerTool::WebFetch])
    );
}

#[test]
fn an_unknown_server_tool_is_refused_naming_the_known_ones() {
    let Err(Malformed::FrontMatter(reason)) = tools_of("server_tools = [\"web_serch\"]\n") else {
        panic!("an unknown server tool is refused");
    };
    for named in ["web_serch", "web_search", "web_fetch"] {
        assert!(reason.contains(named), "{named}: {reason}");
    }
}

#[test]
fn server_tools_write_and_parse_back() {
    let plain = Config {
        model: "sonnet".into(),
        max_tokens: 64,
        wait: None,
        thinking: None,
        server_tools: Vec::new(),
        listen: Vec::new(),
        settle: None,
        compact_at: None,
        system: "Search.".into(),
    };
    assert!(!plain.write(&Pin::default()).contains("server_tools"));
    let both = Config {
        server_tools: ServerTool::ALL.to_vec(),
        ..plain
    };
    assert!(
        both.write(&Pin::default())
            .contains("server_tools = [\"web_search\", \"web_fetch\"]\n")
    );
    assert_eq!(
        Config::parse(both.write(&Pin::default()).as_bytes()),
        Ok(both)
    );
}
