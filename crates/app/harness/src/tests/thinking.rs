use super::*;

#[test]
fn the_thinking_level_the_configuration_names_reaches_the_model_and_its_thinking_comes_back() {
    let world = world();
    let chain = &world.chain;
    let deep = Config {
        thinking: Some(Thinking::High),
        ..scout()
    };
    commit(chain, ADA, CONFIG, &md(&deep));
    let thread = world.opened("m1", "think about it", Some(WINDOW));
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request);
    assert_eq!(sent["thinking"], json!({ "type": "adaptive" }));
    assert_eq!(sent["output_config"], json!({ "effort": "high" }));
    chain.at(3, 0);
    model_says(
        chain,
        json!([
            { "type": "thinking", "thinking": "weighing it", "signature": "sig-1" },
            { "type": "text", "text": "Done." },
        ]),
        "end_turn",
    );
    world.mention("m2", "and now?");
    let (request, _) = open_request(chain);
    let sent = provider_body(chain, &request);
    let turn = &sent["messages"][1]["content"][0];
    assert_eq!(
        (&turn["type"], &turn["thinking"], &turn["signature"]),
        (&json!("thinking"), &json!("weighing it"), &json!("sig-1"))
    );
    assert_eq!(thread_row(chain, thread).frames, 3);
}
