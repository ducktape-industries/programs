use model_wire::{Request, codex, fit};

#[test]
fn summary_instruction_survives_fitting_and_is_absent_by_default() {
    let legacy = serde_json::json!({"model":"test", "max_tokens":1, "messages":[]});
    let default: Request = serde_json::from_value(legacy).unwrap();
    assert_eq!(default.compaction_instruction, None);
    assert!(
        serde_json::to_value(&default)
            .unwrap()
            .get("compaction_instruction")
            .is_none()
    );
    let asked = Request {
        compaction_instruction: Some("Keep \"decisions\".\n한국어".into()),
        ..default
    };
    let fitted = fit::fit(&asked, codex::PROVIDER);
    assert_eq!(fitted.compaction_instruction, asked.compaction_instruction);
    assert_eq!(
        serde_json::from_value::<Request>(serde_json::to_value(&fitted).unwrap()).unwrap(),
        fitted
    );
}
