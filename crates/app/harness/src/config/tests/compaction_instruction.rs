use super::*;

fn prompt(instruction: Option<&str>) -> PromptConfig {
    PromptConfig {
        config: Config::parse(SCOUT.as_bytes()).unwrap(),
        pin: Pin {
            provider: Some("openai".into()),
            service: None,
        },
        compaction_instruction: instruction.map(str::to_owned),
    }
}

#[test]
fn a_directive_round_trips_without_entering_the_public_config() {
    let directive = "  Keep \"decisions\".\n한국어로.  ";
    let parsed = prompt(Some(directive));
    let file = parsed.write();
    assert_eq!(PromptConfig::parse(file.as_bytes()), Ok(parsed.clone()));
    assert_eq!(Config::parse(file.as_bytes()), Ok(parsed.config.clone()));
    assert_eq!(
        borsh::to_vec(&parsed.config).unwrap(),
        borsh::to_vec(&prompt(None).config).unwrap()
    );
    let absent = prompt(None).write();
    assert!(!absent.contains("compaction_instruction"));
    assert_eq!(
        PromptConfig::parse(absent.as_bytes())
            .unwrap()
            .compaction_instruction,
        None
    );
}

#[test]
fn file_rewrites_preserve_the_directive_and_the_ordinary_body() {
    let parsed = prompt(Some("Keep the plan.\nKeep exact paths."));
    let file = parsed.write();
    let mut model = parsed.config.clone();
    model.model = "next-model".into();
    let rewritten = with_front(file.as_bytes(), &model, "openai").unwrap();
    let read = PromptConfig::parse(rewritten.as_bytes()).unwrap();
    assert_eq!(read.compaction_instruction, parsed.compaction_instruction);
    assert_eq!(read.config.model, "next-model");
    assert_eq!(read.config.system, parsed.config.system);
    let rewritten = with_body(rewritten.as_bytes(), "New ordinary prompt.").unwrap();
    let read = PromptConfig::parse(rewritten.as_bytes()).unwrap();
    assert_eq!(read.compaction_instruction, parsed.compaction_instruction);
    assert_eq!(read.config.system, "New ordinary prompt.");
}

#[test]
fn blank_or_non_string_directives_are_malformed() {
    for value in ["\"\"", "\" \\n\\t\"", "7", "true"] {
        let file = SCOUT.replacen(
            "+++\n",
            &format!("+++\ncompaction_instruction = {value}\n"),
            1,
        );
        assert!(matches!(
            PromptConfig::parse(file.as_bytes()),
            Err(Malformed::FrontMatter(_))
        ));
    }
}
