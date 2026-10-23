use super::*;
use crate::Thinking;

fn sent(thinking: Option<Thinking>) -> Value {
    let system = ["Be brief.".to_owned()];
    let header = Header {
        name: "claude-test",
        max_tokens: 64,
        thinking,
        server_tools: &[],
        system: &system,
        file_tools: &[],
        cache_key: "7",
    };
    let mut text = super::header(Wire::AnthropicMessages, &header);
    text.push_str(anthropic::CLOSE);
    serde_json::from_str(&text).expect("the header and the close make one JSON request")
}

#[test]
fn the_anthropic_wire_maps_each_thinking_level() {
    let default = sent(None);
    assert!(default.get("thinking").is_none(), "{default}");
    assert!(default.get("output_config").is_none(), "{default}");

    let off = sent(Some(Thinking::Off));
    assert_eq!(off["thinking"], json!({ "type": "disabled" }));
    assert!(off.get("output_config").is_none(), "{off}");

    for (level, effort) in [
        (Thinking::Low, "low"),
        (Thinking::Medium, "medium"),
        (Thinking::High, "high"),
        (Thinking::Max, "max"),
    ] {
        let thinks = sent(Some(level));
        assert_eq!(thinks["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(thinks["output_config"], json!({ "effort": effort }));
    }
}
