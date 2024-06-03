use anyhow::Result;
use gpui::AppContext;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::SettingsSources;

#[derive(Clone)]
pub struct I18nSettings {
    /// Set the UI language, default is "en".
    pub locale: String,
}

impl I18nSettings {
    /// Switches to the locale and reloads the translations.
    pub fn switch_locale(&mut self, locale: &str, _cx: &mut AppContext) -> Option<String> {
        let mut new_locale = None;

        if crate::BACKEND_DATA.available_locales().any(|l| l == locale) {
            self.locale = locale.to_string();
            new_locale = Some(locale.to_string());
        }

        new_locale
    }
}
/// Settings for rendering text in UI and text buffers.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct I18nSettingsContent {
    /// The default font size for text in the UI.
    #[serde(default)]
    pub locale: Option<String>,
}

impl settings::Settings for I18nSettings {
    const KEY: Option<&'static str> = None;

    type FileContent = I18nSettingsContent;

    fn load(sources: SettingsSources<Self::FileContent>, _cx: &mut AppContext) -> Result<Self> {
        let defaults = sources.default;
        let mut this = Self {
            locale: defaults.locale.clone().unwrap(),
        };

        for value in sources.user.into_iter().chain(sources.release_channel) {
            if let Some(locale) = &value.locale {
                this.locale = locale.to_string();
            }
        }

        Ok(this)
    }
}
