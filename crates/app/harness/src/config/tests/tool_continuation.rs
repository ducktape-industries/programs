use super::*;

#[test]
fn continuation_defaults_to_async_and_sync_survives_prompt_rewrites() {
    let plain = PromptConfig::parse(SCOUT.as_bytes()).unwrap();
    assert_eq!(plain.tool_continuation, ToolContinuation::Async);
    assert!(!plain.write().contains("tool_continuation"));
    let sync = PromptConfig {
        tool_continuation: ToolContinuation::Sync,
        ..plain.clone()
    };
    let file = sync.write();
    assert_eq!(PromptConfig::parse(file.as_bytes()), Ok(sync.clone()));
    assert_eq!(
        borsh::to_vec(&sync.config).unwrap(),
        borsh::to_vec(&plain.config).unwrap()
    );
    let front = with_front(file.as_bytes(), &sync.config, "another").unwrap();
    let body = with_body(front.as_bytes(), "New body").unwrap();
    assert_eq!(
        PromptConfig::parse(body.as_bytes())
            .unwrap()
            .tool_continuation,
        ToolContinuation::Sync
    );
    for value in ["\"unknown\"", "true", "1"] {
        let file = SCOUT.replacen("+++\n", &format!("+++\ntool_continuation = {value}\n"), 1);
        assert!(matches!(
            PromptConfig::parse(file.as_bytes()),
            Err(Malformed::FrontMatter(_))
        ));
    }
}
