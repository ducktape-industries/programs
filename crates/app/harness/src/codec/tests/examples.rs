use ::program::schema::interface;
use borsh::schema::BorshSchemaContainer;

use super::*;

fn written_back(program: &str, schema: BorshSchemaContainer) -> usize {
    let roots = roots(schema);
    let mut made = 0;
    for root in [&roots.op, &roots.query] {
        for (variant, value) in borsh_json::examples(&roots.schema, root).unwrap() {
            let bytes = encode(&roots.schema, root, "value", &value)
                .unwrap_or_else(|fault| panic!("{program} {variant}: {fault}"));
            let read = decode(&roots.schema, root, "value", &bytes).unwrap();
            assert_eq!(read, value, "{program} {variant}");
            made += 1;
        }
    }
    made
}

#[test]
fn every_op_and_query_of_every_program_has_an_example_the_codec_writes_and_reads_back() {
    let interfaces = [
        ("chat", interface::<chat::Chat>()),
        ("access", interface::<access::Access>()),
        ("files", interface::<files::Files>()),
        ("forge", interface::<forge::Forge>()),
        ("identity", interface::<identity::Identity>()),
        ("programs", interface::<::programs::Programs>()),
        ("valset", interface::<valset::Valset>()),
        ("blobs", interface::<blobs::Blobs>()),
        ("awaits", interface::<awaits::Awaits>()),
        ("halo", interface::<halo::Halo>()),
        ("scheduler", interface::<scheduler::Scheduler>()),
        ("activity", interface::<activity::Activity>()),
        ("egress", interface::<egress::Egress>()),
        ("attribution", interface::<attribution::Attribution>()),
        ("harness", interface::<crate::Harness>()),
    ];
    for (program, schema) in interfaces {
        assert!(written_back(program, schema) > 0, "{program}");
    }
}
