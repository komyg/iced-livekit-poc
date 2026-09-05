use dotenvy::dotenv;
use rust_i18n::available_locales;
use std::env::var;

/// Switches the UI to the language `LANG` names — a bare `en`, `es`, `it` or
/// `pt`, not a POSIX tag. Anything unknown leaves the `i18n!` fallback in
/// place.
pub fn set_locale_from_env() {
    let Ok(language) = var("LANG") else {
        return;
    };
    let language = language.trim().to_lowercase();

    if available_locales!().iter().any(|known| *known == language) {
        rust_i18n::set_locale(&language);
    }
}

#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_field_names)]
pub struct ApiKey {
    pub api_key: String,
    pub api_secret: String,
    pub api_url: String,
    pub identity: String,
    pub room: String,
}

impl ApiKey {
    pub fn from_env() -> Self {
        if let Err(error) = dotenv() {
            if error.not_found() {
                eprintln!("warning: no .env file found, falling back to the environment");
            } else {
                eprintln!("warning: failed to load .env: {error}");
            }
        }

        Self {
            api_key: var("LIVEKIT_API_KEY").unwrap_or_default(),
            api_secret: var("LIVEKIT_API_SECRET").unwrap_or_default(),
            api_url: var("LIVEKIT_URL").unwrap_or_default(),
            identity: var("LIVEKIT_IDENTITY").unwrap_or_default(),
            room: var("LIVEKIT_ROOM").unwrap_or_default(),
        }
    }
}
