use ::settings::Settings;
use ::settings::SettingsStore;
use gpui::AppContext;
use gpui::SharedString;
use once_cell::sync::Lazy;
use settings::I18nSettings;
use std::collections::HashMap;
use util::ResultExt;

mod settings;

pub fn init(cx: &mut AppContext) {
    settings::I18nSettings::register(cx);

    let previus_locale = I18nSettings::get_global(cx).locale.clone();
    rust_i18n::set_locale(&previus_locale);

    cx.observe_global::<SettingsStore>(move |cx| {
        let locale = I18nSettings::get_global(cx).locale.clone();
        if previus_locale != locale {
            rust_i18n::set_locale(&locale);
        }
    })
    .detach();
}

pub static I18N_DATA: Lazy<BackendData> = Lazy::new(BackendData::init);

type Translations = HashMap<SharedString, HashMap<SharedString, SharedString>>;

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
                    .expect("invalid locale filename, expected like `en.yml`")
                    .to_string_lossy()
                    .to_string();

                if let Some(asset) = assets::Assets::get(&f) {
                    if let Some(data) =
                        serde_yml::from_slice::<HashMap<SharedString, SharedString>>(&asset.data)
                            .log_err()
                    {
                        trs.insert(locale.into(), data);
                    }
                }
            });

        // TODO: Load from Zed workdir for runtime translation install

        Self { trs }
    }

    pub fn get_text(&self, key: &'static str) -> SharedString {
        let locale = rust_i18n::locale();
        if let Some(s) = self.trs.get(&*locale).and_then(|trs| trs.get(key)) {
            return s.clone();
        }

        SharedString::from(key)
    }
}

#[macro_export]
#[allow(clippy::crate_in_macro_def)]
macro_rules! t {
    // t!("foo")
    ($key:expr) => {
        $crate::I18N_DATA.get_text($key)
    };

    // t!("foo", a = 1, b = "Foo")
    ($key:expr, $($var_name:tt = $var_val:expr),+ $(,)?) => {
        let message = $crate::I18N_DATA.get_text($key);
        let patterns: &[&str] = &[
            $(rust_i18n::key!($var_name)),+
        ];
        let values = &[
            $(format!("{}", $var_val)),+
        ];

        let output = rust_i18n::replace_patterns(message.as_ref(), patterns, values);
        gpui::SharedString::from(output)
    };
}
