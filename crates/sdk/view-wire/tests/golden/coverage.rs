//! A completeness gate the byte fixtures owe the schema: `schema.txt` pins
//! every field and variant the wire reaches, but `frame.bin` and
//! `methods.bin` only pin the bytes of what their samples build. This closes
//! that gap mechanically, off the definitions `schema.rs` already builds --
//! never a hand-kept list, which drifts the moment someone adds a variant
//! and forgets it.
//!
//! THE TREE RULE. Every variant of every enum reachable from `Frame`,
//! `Event` and `WidgetCommand` must be entered at least once, unit variants
//! included: MessagePack writes a unit variant by its name, and a
//! serialize-side rename of one no sample writes moves no byte and no line
//! of `schema.txt` (traced from the `Deserialize` side). Every struct field
//! (a plain struct's, or a struct-shaped variant's) must be seen PRESENT
//! and non-null at least once, the same requirement for a required field
//! and an `Option`/`skip_serializing_if` one. Expected is read off
//! `schema::wire()`, the tree's registry with gpui's `GridLocation` (reached
//! from `StyleRefinement.grid_location`) merged back. Observed is read off
//! each sample's JSON twin -- not `serde_reflection::trace_value`, which
//! unifies repeat containers by exact field count and refuses the moment two
//! `Aria`s skip different fields; JSON keeps field names and represents a
//! skipped field the same as an explicit `null`, which is exactly "not
//! present" either way. A name the walk reaches that the registry lacks, a
//! tag it does not declare, or a twin that does not fit its format fails by
//! name rather than passing unseen.
//!
//! THE BORSH RULE is the test at the bottom: every variant of every enum a
//! method's request or reply reaches, read off the exchanges' own bytes.
use std::collections::{BTreeMap, BTreeSet};

use borsh::schema::{Declaration, Definition, Fields};
use serde_json::Value as Json;
use serde_reflection::{ContainerFormat, Format, FormatHolder, Named, Registry, VariantFormat};

use super::schema::{Definitions, Shape, shapes, wire};

fn container<'a>(registry: &'a Registry, name: &str) -> &'a ContainerFormat {
    registry
        .get(name)
        .unwrap_or_else(|| panic!("`{name}` is on the wire but not in the registry"))
}

fn reachable(registry: &Registry, roots: &[&str]) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = roots.iter().map(|name| (*name).to_string()).collect();
    while let Some(name) = stack.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        container(registry, &name)
            .visit(&mut |format| {
                if let Format::TypeName(name) = format {
                    stack.push(name.clone());
                }
                Ok(())
            })
            .unwrap();
    }
    seen
}

#[derive(Default)]
struct Shown {
    variants: BTreeSet<(String, String)>,
    fields: BTreeSet<String>,
}

fn expected(registry: &Registry, reachable: &BTreeSet<String>) -> Shown {
    let mut out = Shown::default();
    for name in reachable {
        match container(registry, name) {
            ContainerFormat::Struct(named) => {
                for field in named {
                    out.fields.insert(format!("{name}.{}", field.name));
                }
            }
            ContainerFormat::Enum(variants) => {
                for variant in variants.values() {
                    out.variants.insert((name.clone(), variant.name.clone()));
                    if let VariantFormat::Struct(named) = &variant.value {
                        for field in named {
                            out.fields
                                .insert(format!("{name}::{}.{}", variant.name, field.name));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn observe(registry: &Registry, name: &str, value: &Json, out: &mut Shown) {
    if value.is_null() {
        return;
    }
    let misfit = || panic!("`{name}`'s twin does not fit it: {value}");
    match container(registry, name) {
        ContainerFormat::Struct(named) => {
            let Json::Object(map) = value else { misfit() };
            observe_fields(registry, name, named, map, out);
        }
        ContainerFormat::Enum(variants) => {
            let (tag, inner) = match value {
                Json::String(tag) => (tag, &Json::Null),
                Json::Object(map) if map.len() == 1 => map.iter().next().unwrap(),
                _ => misfit(),
            };
            let variant = variants
                .values()
                .find(|variant| &variant.name == tag)
                .unwrap_or_else(|| panic!("`{name}` has no variant `{tag}`"));
            out.variants.insert((name.to_owned(), tag.clone()));
            match &variant.value {
                VariantFormat::Unit | VariantFormat::Variable(_) => {}
                VariantFormat::NewType(format) => observe_format(registry, format, inner, out),
                VariantFormat::Tuple(formats) => observe_tuple(registry, formats, inner, out),
                VariantFormat::Struct(named) => {
                    let Json::Object(map) = inner else { misfit() };
                    observe_fields(registry, &format!("{name}::{tag}"), named, map, out);
                }
            }
        }
        ContainerFormat::NewTypeStruct(format) => observe_format(registry, format, value, out),
        ContainerFormat::TupleStruct(formats) => observe_tuple(registry, formats, value, out),
        ContainerFormat::UnitStruct => {}
    }
}

fn observe_fields(
    registry: &Registry,
    path: &str,
    named: &[Named<Format>],
    map: &serde_json::Map<String, Json>,
    out: &mut Shown,
) {
    for field in named {
        let child = map.get(&field.name).unwrap_or(&Json::Null);
        if !child.is_null() {
            out.fields.insert(format!("{path}.{}", field.name));
        }
        observe_format(registry, &field.value, child, out);
    }
}

fn observe_tuple(registry: &Registry, formats: &[Format], value: &Json, out: &mut Shown) {
    let items = elements(value);
    assert_eq!(items.len(), formats.len(), "a tuple's twin: {value}");
    for (format, item) in formats.iter().zip(items) {
        observe_format(registry, format, item, out);
    }
}

fn observe_format(registry: &Registry, format: &Format, value: &Json, out: &mut Shown) {
    if value.is_null() {
        return;
    }
    match format {
        Format::TypeName(name) => observe(registry, name, value, out),
        Format::Option(inner) => observe_format(registry, inner, value, out),
        Format::Seq(inner) | Format::TupleArray { content: inner, .. } => {
            for item in elements(value) {
                observe_format(registry, inner, item, out);
            }
        }
        Format::Map { value: inner, .. } => {
            let Json::Object(map) = value else {
                panic!("a map's twin: {value}")
            };
            for item in map.values() {
                observe_format(registry, inner, item, out);
            }
        }
        Format::Tuple(formats) => observe_tuple(registry, formats, value, out),
        _ => {}
    }
}

fn missing(expected: &Shown, observed: &Shown) -> Vec<String> {
    let mut gaps: Vec<String> = expected
        .variants
        .difference(&observed.variants)
        .map(|(container, variant)| format!("variant {container}::{variant}"))
        .chain(
            expected
                .fields
                .difference(&observed.fields)
                .map(|field| format!("field {field}")),
        )
        .collect();
    gaps.sort();
    gaps
}

fn elements(value: &Json) -> &[Json] {
    match value {
        Json::Array(items) => items,
        _ => panic!("not an array: {value}"),
    }
}

#[test]
fn every_reachable_variant_and_field_is_sampled() {
    let registry = wire();
    let names = reachable(&registry, &["Frame", "Event", "WidgetCommand"]);
    let expected = expected(&registry, &names);

    let frame = super::events_fixture::every_frame();
    let events = super::events_fixture::every_event();
    let widget_commands = super::widget_fixture::every_widget_command();
    let units = super::units_fixture::every_unit(&registry);

    let mut observed = Shown::default();
    observe(
        &registry,
        "Frame",
        &serde_json::to_value(&frame).unwrap(),
        &mut observed,
    );
    for event in elements(&serde_json::to_value(&events).unwrap()) {
        observe(&registry, "Event", event, &mut observed);
    }
    for command in elements(&serde_json::to_value(&widget_commands).unwrap()) {
        observe(&registry, "WidgetCommand", command, &mut observed);
    }
    let Json::Object(units) = serde_json::to_value(&units).unwrap() else {
        unreachable!("a struct's twin is an object")
    };
    for (name, values) in &units {
        for value in elements(values) {
            observe(&registry, name, value, &mut observed);
        }
    }

    let gaps = missing(&expected, &observed);
    assert!(
        gaps.is_empty(),
        "the golden fixtures do not sample: {gaps:#?}"
    );
}

fn definition<'a>(definitions: &'a Definitions, declaration: &str) -> &'a Definition {
    definitions.get(declaration).unwrap_or_else(|| {
        panic!("`{declaration}` is on the wire but not in the borsh definitions")
    })
}

fn parts(definition: &Definition) -> Vec<&Declaration> {
    match definition {
        Definition::Primitive(_)
        | Definition::Struct {
            fields: Fields::Empty,
        } => vec![],
        Definition::Sequence { elements, .. } => vec![elements],
        Definition::Tuple { elements } => elements.iter().collect(),
        Definition::Enum { variants, .. } => {
            variants.iter().map(|(_, _, variant)| variant).collect()
        }
        Definition::Struct {
            fields: Fields::NamedFields(fields),
        } => fields.iter().map(|(_, field)| field).collect(),
        Definition::Struct {
            fields: Fields::UnnamedFields(fields),
        } => fields.iter().collect(),
    }
}

fn borsh_variants(
    definitions: &Definitions,
    roots: Vec<Declaration>,
) -> BTreeSet<(String, String)> {
    let mut seen = BTreeSet::new();
    let mut stack = roots;
    let mut variants = BTreeSet::new();
    while let Some(declaration) = stack.pop() {
        if !seen.insert(declaration.clone()) {
            continue;
        }
        let definition = definition(definitions, &declaration);
        if let Definition::Enum { variants: each, .. } = definition {
            variants.extend(
                each.iter()
                    .map(|(_, name, _)| (declaration.clone(), name.clone())),
            );
        }
        stack.extend(parts(definition).into_iter().cloned());
    }
    variants
}

fn take<'a>(bytes: &mut &'a [u8], width: usize) -> &'a [u8] {
    let (head, rest) = bytes
        .split_at_checked(width)
        .expect("the bytes end before their definition does");
    *bytes = rest;
    head
}

fn uint(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .rev()
        .fold(0, |value, byte| value << 8 | u64::from(*byte))
}

fn read(
    definitions: &Definitions,
    declaration: &str,
    bytes: &mut &[u8],
    taken: &mut BTreeSet<(String, String)>,
) {
    match definition(definitions, declaration) {
        Definition::Primitive(width) => {
            take(bytes, usize::from(*width));
        }
        Definition::Sequence {
            length_width,
            length_range,
            elements,
        } => {
            let length = match length_width {
                0 => *length_range.start(),
                width => uint(take(bytes, usize::from(*width))),
            };
            for _ in 0..length {
                read(definitions, elements, bytes, taken);
            }
        }
        Definition::Enum {
            tag_width,
            variants,
        } => {
            let tag = uint(take(bytes, usize::from(*tag_width)));
            let (_, name, variant) = variants
                .iter()
                .find(|(discriminant, ..)| u64::try_from(*discriminant) == Ok(tag))
                .unwrap_or_else(|| panic!("`{declaration}` has no variant {tag}"));
            taken.insert((declaration.to_owned(), name.clone()));
            read(definitions, variant, bytes, taken);
        }
        other => {
            for part in parts(other) {
                read(definitions, part, bytes, taken);
            }
        }
    }
}

#[test]
fn every_borsh_variant_is_sampled() {
    let (shapes, definitions) = shapes();
    let shapes: BTreeMap<&str, Shape> = shapes
        .into_iter()
        .filter(|(_, shape)| shape.borsh && shape.target.is_none())
        .collect();
    let roots = shapes
        .values()
        .flat_map(|shape| [shape.request.clone(), shape.reply.clone()])
        .collect();
    let expected = borsh_variants(&definitions, roots);

    let mut taken = BTreeSet::new();
    for ((kind, request, reply), _) in super::every_method() {
        let Some(shape) = shapes.get(kind.as_str()) else {
            continue;
        };
        for (declaration, bytes) in [(&shape.request, request), (&shape.reply, reply)] {
            let mut rest = bytes.as_slice();
            read(&definitions, declaration, &mut rest, &mut taken);
            assert!(rest.is_empty(), "{kind}: bytes left after `{declaration}`");
        }
    }

    let gaps: Vec<String> = expected
        .difference(&taken)
        .map(|(declaration, variant)| format!("variant {declaration}::{variant}"))
        .collect();
    assert!(
        gaps.is_empty(),
        "the golden methods do not sample: {gaps:#?}"
    );
}
