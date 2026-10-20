use super::*;

#[test]
fn a_field_marked_invalid_required_and_read_only_says_so_and_why() {
    let lower = |input: Input| {
        let mut app = App::for_driver();
        let mut window = app.window();
        Lowering::new(&mut window, &mut app).lower(input)
    };
    let email = || {
        Input::new("email", &crate::TextField::default(), "Email")
            .invalid(gpui::accesskit::Invalid::True)
            .required(true)
            .read_only(true)
    };
    let wire::Node::Field { options, .. } = lower(email()) else {
        panic!("an input")
    };
    assert_eq!(options.invalid, Some(gpui::accesskit::Invalid::True));
    assert!(options.required && options.read_only);
    let faults = |node: &wire::Node| {
        wire::audit(node)
            .into_iter()
            .map(|fault| fault.kind)
            .collect::<Vec<_>>()
    };
    assert_eq!(faults(&lower(email())), [wire::FaultKind::ErrorNoText]);
    assert_eq!(
        faults(&lower(email().description("An address has an @"))),
        []
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn an_element_a_builder_method_moves_is_small() {
    macro_rules! grown {
        ($($element:ty: $most:literal),+ $(,)?) => {
            [$((stringify!($element), size_of::<$element>(), $most)),+]
                .into_iter()
                .filter(|(_, size, most)| size > most)
                .collect::<Vec<_>>()
        };
    }
    let grown = grown![
        Div: 32,
        crate::Stateful<Div>: 32,
        crate::Img: 80,
        crate::Stateful<crate::Img>: 80,
        crate::Svg: 64,
        crate::Stateful<crate::Svg>: 64,
        UniformList: 56,
        crate::Stateful<UniformList>: 56,
        crate::ResizeHandle: 48,
        crate::Sensor: 80,
        crate::ModalOverlay: 136,
        Input: 296,
        Textarea: 296,
        crate::List: 80,
        crate::StyledText: 104,
        crate::InteractiveText: 216,
        crate::Canvas: 32,
        crate::Anchored: 72,
        crate::Deferred: 24,
        AnyElement: 16,
    ];
    assert!(
        grown.is_empty(),
        "(element, bytes, the most it may be): {grown:?}"
    );
}
