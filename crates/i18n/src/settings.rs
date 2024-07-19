use anyhow::Result;
use gpui::{AppContext, SharedString};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings::SettingsSources;

#[derive(Clone)]
pub struct I18nSettings {
    /// Set the UI language, default is "en".
    pub locale: SharedString,
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
            locale: defaults.locale.clone().unwrap().into(),
        };

        for value in sources.user.into_iter().chain(sources.release_channel) {
            if let Some(locale) = value.locale.clone() {
                this.locale = locale.into();
            }
        }

        Ok(this)
    }
}
