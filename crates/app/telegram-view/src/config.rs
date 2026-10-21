use grammers_tl_types::{self as tl, Serializable};

const API_ID: i32 = 0;
const API_HASH: &str = "";

pub(crate) fn send_code(phone: String) -> Vec<u8> {
    tl::functions::auth::SendCode {
        phone_number: phone,
        api_id: API_ID,
        api_hash: API_HASH.into(),
        settings: tl::enums::CodeSettings::Settings(tl::types::CodeSettings {
            allow_flashcall: false,
            current_number: false,
            allow_app_hash: false,
            allow_missed_call: false,
            allow_firebase: false,
            unknown_number: false,
            logout_tokens: None,
            token: None,
            app_sandbox: None,
        }),
    }
    .to_bytes()
}

struct Raw<'a>(&'a [u8]);
impl Serializable for Raw<'_> {
    fn serialize(&self, buffer: &mut impl Extend<u8>) {
        buffer.extend(self.0.iter().copied());
    }
}

pub(crate) fn initialize(body: &[u8]) -> Vec<u8> {
    tl::functions::InvokeWithLayer {
        layer: tl::LAYER,
        query: tl::functions::InitConnection {
            api_id: API_ID,
            device_model: "ducktape".into(),
            system_version: "view".into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            system_lang_code: "en".into(),
            lang_pack: String::new(),
            lang_code: "en".into(),
            proxy: None,
            params: None,
            query: Raw(body),
        },
    }
    .to_bytes()
}
