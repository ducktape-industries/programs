use super::*;

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
fn the_namespace_roots_are_the_chains_and_a_home_names_its_holder() {
    for root in ["/home", "/shared", "/home/ada"] {
        let segments = change_path(root).unwrap();
        assert_eq!(
            beneath_the_roots(&segments).unwrap_err().code,
            ::program::code::UNAUTHORIZED,
            "{root}"
        );
    }
    for beneath in ["/home/ada/notes", "/shared/a", "/docs"] {
        assert!(beneath_the_roots(&change_path(beneath).unwrap()).is_ok());
    }
    assert_eq!(is_home(&change_path("/home/ada").unwrap()), Some("ada"));
    assert_eq!(is_home(&change_path("/home/ada/notes").unwrap()), None);
}

#[test]
fn the_chat_folders_down_to_a_message_are_roots_and_its_files_are_not() {
    for folder in ["/chat", "/chat/general", "/chat/general/m1"] {
        let segments = change_path(folder).unwrap();
        assert!(is_chat_folder(&segments), "{folder}");
        assert_eq!(
            beneath_the_roots(&segments).unwrap_err().code,
            ::program::code::UNAUTHORIZED,
            "{folder}"
        );
    }
    for beneath in ["/chat/general/m1/a", "/chat/general/m1/a/b", "/chats/x"] {
        let segments = change_path(beneath).unwrap();
        assert!(!is_chat_folder(&segments), "{beneath}");
        assert!(beneath_the_roots(&segments).is_ok(), "{beneath}");
    }
    assert!(!is_chat_folder(&change_path("/home/ada").unwrap()));
}
