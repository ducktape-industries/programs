use super::*;

#[test]
fn a_streamed_search_and_fetch_fold_into_the_turn_their_whole_reply_reads_as() {
    let citation = json!({
        "type": "web_search_result_location",
        "url": "https://ducktape.example",
        "title": "Ducktape",
        "encrypted_index": "Eo8BCioIAhgB",
        "cited_text": "Ducktape is a network.",
    });
    let content = json!([
        {
            "type": "server_tool_use",
            "id": "srvtoolu_1",
            "name": "web_search",
            "input": { "query": "ducktape" },
        },
        {
            "type": "web_search_tool_result",
            "tool_use_id": "srvtoolu_1",
            "content": [{
                "type": "web_search_result",
                "url": "https://ducktape.example",
                "title": "Ducktape",
                "encrypted_content": "EqgfCioIARgB",
                "page_age": null,
            }],
        },
        {
            "type": "server_tool_use",
            "id": "srvtoolu_2",
            "name": "web_fetch",
            "input": { "url": "https://ducktape.example" },
        },
        {
            "type": "web_fetch_tool_result",
            "tool_use_id": "srvtoolu_2",
            "content": {
                "type": "web_fetch_result",
                "url": "https://ducktape.example",
                "content": {
                    "type": "document",
                    "source": {
                        "type": "text",
                        "media_type": "text/plain",
                        "data": "Ducktape is a network.",
                    },
                    "title": "Ducktape",
                },
                "retrieved_at": "2026-10-18T10:00:00Z",
            },
        },
        { "type": "text", "text": "Ducktape is a network.", "citations": [citation] },
    ]);
    let reply = folded_reply(SERVER_TOOLS);
    assert_eq!(reply, whole(content, "end_turn", 5));
    assert!(matches!(
        &reply.said[..],
        [
            Said::ServerToolUse { .. },
            Said::ServerToolResult(_),
            Said::ServerToolUse { .. },
            Said::ServerToolResult(_),
            Said::Cited { .. },
        ]
    ));
}
