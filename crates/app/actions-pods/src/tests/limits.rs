use super::*;

fn event_at(node: &mut Node, wait: &RequestRow, seq: u64, ms: u64, data: &[u8]) {
    node.after(ms);
    node.node(&Egress::Events(vec![Event {
        id: wait.id,
        seq,
        data: data.to_vec(),
    }]));
}

fn finish_timeout_with_open_wait(node: &mut Node) {
    node.pending_to("POST", "/v1/pods/duckpond-7/exec");
    node.answer_to(
        0,
        "POST",
        "/v1/pods/duckpond-7/exec",
        200,
        json!({"exit_code": 0, "stdout": "still running\n"}),
    );
    node.answer_to(
        0,
        "POST",
        "/v1/pods/duckpond-7/exec",
        200,
        json!({"exit_code": 0, "stdout": "/out/log\n"}),
    );
    node.answer_to(0, "POST", "/v1/pods/duckpond-7/file", 200, json!({}));
    node.answer_to(0, "DELETE", "/v1/pods/duckpond-7", 200, json!({}));
    let reports = node.reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, LIMITED);
    assert_eq!(reports[0].1, State::Failed);
    assert!(reports[0].2.contains("timeout-minutes = 2"));
    assert!(reports[0].3.is_some());
    assert!(node.held().is_empty());
    assert_eq!(progress(node, LIMITED), None);
}

#[test]
fn alive_events_every_minute_do_not_extend_a_two_minute_check() {
    let mut node = node();
    let wait = waiting_on_check(&mut node, LIMITED);
    let alive = b"event: alive\ndata: {}";
    event_at(&mut node, &wait, 0, 60_000, alive);
    assert_eq!(node.open().len(), 1);
    assert!(node.reports().is_empty());
    event_at(&mut node, &wait, 1, 60_000, alive);
    let tail = node.pending_to("POST", "/v1/pods/duckpond-7/exec");
    assert!(
        node.body(&tail)["command"][2]
            .as_str()
            .unwrap()
            .starts_with("tail ")
    );
    event_at(&mut node, &wait, 2, 0, alive);
    assert_eq!(
        node.open().len(),
        2,
        "a second event starts no second cleanup"
    );
    node.node(&Egress::EndStream {
        id: wait.id,
        events: 3,
        end: StreamEnd::Complete {
            status: 200,
            headers: Vec::new(),
        },
    });
    assert_eq!(
        node.open().len(),
        1,
        "the old wait starts no pod read or cleanup"
    );
    finish_timeout_with_open_wait(&mut node);
    assert!(node.idle());
    assert!(node.reports()[0].2.starts_with("ran 120 s, "));
    let run = node.run(LIMITED).unwrap();
    assert_eq!(run.pod.as_deref(), Some("duckpond-7"));
    assert!(
        run.every > 0,
        "event completion keeps the active run's measurements"
    );
}

#[test]
fn an_unreadable_event_still_checks_the_limit_and_a_late_end_does_not_report_again() {
    let mut node = node();
    let wait = waiting_on_check(&mut node, LIMITED);
    event_at(&mut node, &wait, 0, 120_000, b"\xff");
    finish_timeout_with_open_wait(&mut node);
    node.ended(&wait, 1, json!(0));
    assert_eq!(node.reports().len(), 1);
    assert!(node.idle());
    assert_eq!(progress(&node, LIMITED), None);
}

#[test]
fn alive_events_do_not_give_an_unlimited_check_a_limit() {
    let mut node = node();
    let wait = waiting_on_check(&mut node, 1);
    for seq in 0..3 {
        event_at(&mut node, &wait, seq, 60_000, b"event: alive\ndata: {}");
        assert_eq!(node.open().len(), 1);
        assert!(node.reports().is_empty());
    }
    node.ended(&wait, 3, json!(0));
    copied_and_deleted(&mut node);
    assert_eq!(node.reports()[0].1, State::Passed);
}

#[test]
fn a_progress_data_field_needs_no_space_after_its_colon() {
    let mut node = node();
    let wait = waiting_on_check(&mut node, 1);
    event_at(
        &mut node,
        &wait,
        0,
        1_000,
        b"event: alive\ndata:{\"log\":{\"bytes\":12,\"line\":\"working\"}}",
    );
    assert_eq!(
        progress(&node, 1),
        read_now(
            &node,
            LogRead::Read {
                bytes: 12,
                line: "working".into()
            }
        )
    );
}

#[test]
fn an_ended_data_field_needs_no_space_after_its_colon() {
    let mut node = node();
    let wait = waiting_on_check(&mut node, 1);
    event_at(
        &mut node,
        &wait,
        0,
        1_000,
        b"event: ended\ndata:{\"exit_code\":0,\"line\":\"finished\"}",
    );
    node.node(&Egress::EndStream {
        id: wait.id,
        events: 1,
        end: StreamEnd::Complete {
            status: 200,
            headers: Vec::new(),
        },
    });
    copied_and_deleted(&mut node);
    assert_eq!(node.reports()[0].1, State::Passed);
    assert_eq!(node.reports()[0].2, "finished");
}
