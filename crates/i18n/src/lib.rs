use ::settings::Settings;
use ::settings::SettingsStore;
use gpui::AppContext;
use once_cell::sync::Lazy;
use settings::I18nSettings;
use std::collections::HashMap;
use util::ResultExt;

mod settings;

static BACKEND_DATA: Lazy<BackendData> = Lazy::new(BackendData::init);

type Translations = HashMap<String, HashMap<String, String>>;

pub struct BackendData {
    trs: Translations,
}

impl BackendData {
    pub fn init() -> Self {
        let mut trs = Translations::default();

        // Load all translations from assets/locales/*.yml
        assets::Assets::iter()
            .filter(|f| f.starts_with("locales/") && f.ends_with(".yml"))
            .for_each(|f| {
                let locale = std::path::Path::new(f.as_ref())
                    .file_stem()
                    .expect("invalid locale filename, no extension, expected like `en.yml`")
                    .to_string_lossy()
                    .to_string();

                if let Some(asset) = assets::Assets::get(&f) {
                    if let Some(data) =
                        serde_yaml::from_slice::<HashMap<String, String>>(&asset.data).log_err()
                    {
                        trs.insert(locale, data);
                    }
                }
            });

        // TODO: Load from Zed workdir for runtime translation install

        Self { trs }
    }

    pub fn available_locales(&self) -> impl Iterator<Item = &str> {
        self.trs.keys().map(|s| s.as_str())
    }
}

#[derive(Default)]
pub struct Backend {}

impl rust_i18n::Backend for Backend {
    fn available_locales(&self) -> Vec<&str> {
        BACKEND_DATA.trs.keys().map(|s| s.as_str()).collect()
    }

    fn translate(&self, locale: &str, key: &str) -> Option<&str> {
        if let Some(trs) = BACKEND_DATA.trs.get(locale) {
            trs.get(key).map(|s| s.as_str())
        } else {
            None
        }
    }
}

/// Initialize the i18n system for current crate.
#[macro_export]
macro_rules! init {
    () => {
        rust_i18n::i18n!(fallback = "en", backend = i18n::Backend::default());
    };
}

pub fn init(cx: &mut AppContext) {
    settings::I18nSettings::register(cx);

    let previus_locale = I18nSettings::get_global(cx).locale.clone();
    cx.observe_global::<SettingsStore>(move |cx| {
        let locale = I18nSettings::get_global(cx).locale.clone();
        if previus_locale != locale {
            // TODO: Dispatch Notification to tell use to restart Zed to apply new locale
        }
    })
    .detach();
}

pub use rust_i18n::available_locales;
pub use rust_i18n::set_locale;

#[macro_export]
#[allow(clippy::crate_in_macro_def)]
macro_rules! t {
    // t!("foo")
    ($key:expr) => {
        gpui::SharedString::from(rust_i18n::t!($key))
    };

    // t!("foo", a = 1, b = "Foo")
    ($key:expr, $($var_name:tt = $var_val:expr),+ $(,)?) => {
        gpui::SharedString::from(rust_i18n::t!($key, $($var_name = $var_val),+))
    };
}
