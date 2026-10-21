use super::*;

fn founded() -> Roots {
    Roots::new(vec![
        Root {
            name: "home".into(),
            depth: 1,
            owner: Some(Owner::Named),
        },
        Root {
            name: "shared".into(),
            depth: 0,
            owner: None,
        },
        Root {
            name: "chat".into(),
            depth: 2,
            owner: Some(Owner::Program("chat".into())),
        },
    ])
}

fn at(path: &str) -> Vec<String> {
    change_path(path).unwrap()
}

#[test]
fn a_path_is_absolute_and_composed() {
    assert_eq!(segments("/").unwrap(), Vec::<String>::new());
    assert_eq!(segments("/shared/a").unwrap(), ["shared", "a"]);
    for bad in [
        "shared",
        "/shared//a",
        "/shared/a/",
        "/shared/./a",
        "/shared/../a",
        "/a\0b",
        "/cafe\u{301}",
    ] {
        assert_eq!(
            segments(bad).unwrap_err().code,
            ::program::code::INVALID_INPUT,
            "{bad:?}"
        );
    }
    assert_eq!(
        change_path("/").unwrap_err().code,
        ::program::code::INVALID_INPUT
    );
}

#[test]
fn the_namespace_roots_are_the_ones_the_params_name_and_a_home_names_its_holder() {
    let roots = founded();
    for root in ["/home", "/shared", "/home/ada"] {
        assert_eq!(
            roots.beneath(&at(root)).unwrap_err().code,
            ::program::code::UNAUTHORIZED,
            "{root}"
        );
    }
    for beneath in ["/home/ada/notes", "/shared/a", "/docs"] {
        assert!(roots.beneath(&at(beneath)).is_ok(), "{beneath}");
    }
    assert_eq!(roots.keeper(&at("/home/ada")), Some(Keeper::Named("ada")));
    assert_eq!(roots.keeper(&at("/home/ada/notes")), None);
    assert_eq!(roots.keeper(&at("/home")), None);
    assert_eq!(roots.keeper(&at("/shared")), None);
    assert!(roots.holds_namespace_roots(&at("/home")));
    for folder in ["/shared", "/home/ada", "/docs"] {
        assert!(!roots.holds_namespace_roots(&at(folder)), "{folder}");
    }
}

#[test]
fn a_programs_folders_down_to_its_depth_are_its_roots_and_what_is_beneath_is_carried() {
    let roots = founded();
    for folder in ["/chat", "/chat/general", "/chat/general/m1"] {
        assert_eq!(
            roots.keeper(&at(folder)),
            Some(Keeper::Program("chat")),
            "{folder}"
        );
        assert_eq!(
            roots.beneath(&at(folder)).unwrap_err().code,
            ::program::code::UNAUTHORIZED,
            "{folder}"
        );
        assert!(!roots.carried(&at(folder)), "{folder}");
    }
    for beneath in ["/chat/general/m1/a", "/chat/general/m1/a/b"] {
        assert_eq!(roots.keeper(&at(beneath)), None, "{beneath}");
        assert!(roots.beneath(&at(beneath)).is_ok(), "{beneath}");
        assert!(roots.carried(&at(beneath)), "{beneath}");
    }
    for elsewhere in ["/chats/x/y/z", "/home/ada/x/y/z", "/shared/a/b/c"] {
        assert_eq!(roots.keeper(&at(elsewhere)), None, "{elsewhere}");
        assert!(!roots.carried(&at(elsewhere)), "{elsewhere}");
    }
}

#[test]
fn the_roots_read_from_params_and_none_named_means_none() {
    assert_eq!(Roots::of(&[]).unwrap(), Roots::default());
    assert!(Roots::default().beneath(&at("/home")).is_ok());
    let named = abi::encode(&vec![Root {
        name: "srv".into(),
        depth: 1,
        owner: Some(Owner::Program("forge".into())),
    }]);
    let roots = Roots::of(&named).unwrap();
    assert!(roots.beneath(&at("/srv/site")).is_err());
    assert!(roots.beneath(&at("/srv/site/index.html")).is_ok());
    assert!(roots.beneath(&at("/home")).is_ok());
    assert_eq!(
        roots.keeper(&at("/srv/site")),
        Some(Keeper::Program("forge"))
    );
    assert_eq!(
        Roots::of(b"\x07").unwrap_err().code,
        ::program::code::INVALID_INPUT
    );
}
