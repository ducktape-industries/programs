//! What a view's `key.sign` signs (J-389): the seated key signs under the one namespace `VIEW_SIGN`
//! over borsh(view id, purpose, message), so no view picks the namespace or another view's bytes, and
//! a program checks the signature with `verify` over the same bytes.
pub const VIEW_SIGN: &str = "ducktape/view-sign/v1";

pub fn statement(view: &str, purpose: &str, message: &[u8]) -> Vec<u8> {
    abi::encode(&(view.to_owned(), purpose.to_owned(), message.to_vec()))
}

pub fn prefix_related(namespace: &[u8]) -> bool {
    let ours = VIEW_SIGN.as_bytes();
    namespace.starts_with(ours) || ours.starts_with(namespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_statement_is_each_field_length_prefixed_in_order() {
        let mut want = vec![9, 0, 0, 0];
        want.extend(b"chat-view");
        want.extend([14, 0, 0, 0]);
        want.extend(b"seal-device/v1");
        want.extend([2, 0, 0, 0, 1, 2]);
        assert_eq!(statement("chat-view", "seal-device/v1", &[1, 2]), want);
    }

    #[test]
    fn no_view_signs_another_views_statement_by_moving_bytes_between_id_and_purpose() {
        let message = b"key".as_slice();
        assert_ne!(
            statement("chat-view/seal-device", "v1", message),
            statement("chat-view", "seal-device/v1", message)
        );
        assert_ne!(
            statement("owner/name", "x", message),
            statement("owner", "name/x", message)
        );
    }

    #[test]
    fn a_namespace_that_shares_a_prefix_with_view_sign_is_told_apart() {
        for related in [
            b"ducktape/view-sign/v1".as_slice(),
            b"ducktape/view-sign/v1x",
            b"ducktape",
            b"",
        ] {
            assert!(prefix_related(related), "{related:?}");
        }
        for apart in [
            b"ducktape:identity:consent".as_slice(),
            b"duckpond-725bcad1",
            b"ducktape/view-sign/v2",
        ] {
            assert!(!prefix_related(apart), "{apart:?}");
        }
    }
}
