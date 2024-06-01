use std::collections::HashMap;
use util::ResultExt;

rust_i18n::i18n!(fallback = "en");

type Translations = HashMap<String, HashMap<String, String>>;

pub struct Backend {
    trs: Translations,
}

impl Backend {
    pub fn reload() -> Self {
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

                dbg!(f.clone());
                if let Some(asset) = assets::Assets::get(&f) {
                    if let Some(data) =
                        serde_yaml::from_slice::<HashMap<String, String>>(&asset.data).log_err()
                    {
                        dbg!("install trs", locale.clone());
                        trs.insert(locale, data);
                    }
                }
            });

        // TODO: Load from Zed workdir for runtime translation install

        Self { trs }
    }
}

impl rust_i18n::Backend for Backend {
    fn available_locales(&self) -> Vec<&str> {
        self.trs.keys().map(|s| s.as_str()).collect()
    }

    fn translate<'a>(&'a self, locale: &str, key: &str) -> Option<&str> {
        if let Some(trs) = self.trs.get(locale) {
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
        rust_i18n::i18n!(fallback = "en", backend = i18n::Backend::reload());
    };
}

pub use rust_i18n::set_locale;
pub use rust_i18n::t;
