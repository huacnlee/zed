use std::sync::Arc;

use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    size,
};
use language::LanguageRegistry;
use markdown_preview::{
    markdown_elements::ParsedMarkdown, markdown_parser::parse_markdown,
    markdown_renderer::render_parsed_markdown,
};
use node_runtime::NodeRuntime;
use settings::Settings as _;
use theme::ThemeSettings;
use ui::{ActiveTheme as _, relative};

use std::borrow::Cow;

use anyhow::{Context as _, Result};
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
#[include = "themes/**/*"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Self::get(path)
            .map(|f| f.data)
            .with_context(|| format!("could not find asset at path {path:?}"))
            .map(Some)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

struct MarkdownPreviewExample {
    parsed_markdown: ParsedMarkdown,
}

impl Render for MarkdownPreviewExample {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let buffer_size = ThemeSettings::get_global(cx).buffer_font_size(cx);
        let buffer_line_height = ThemeSettings::get_global(cx).buffer_line_height;

        div()
            .id("markdown-preview")
            .size_full()
            .overflow_y_scroll()
            .size_full()
            .bg(cx.theme().colors().editor_background)
            .text_color(cx.theme().colors().text)
            .p_4()
            .text_size(buffer_size)
            .line_height(relative(buffer_line_height.value()))
            .child(render_parsed_markdown(
                &self.parsed_markdown,
                None,
                window,
                cx,
            ))
    }
}

fn main() {
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        settings::init(cx);
        theme::init(theme::LoadThemes::All(Box::new(Assets)), cx);
        language::init(cx);

        let mut theme_settings = ThemeSettings::get_global(cx).clone();

        let node_runtime = NodeRuntime::unavailable();
        let fs = fs::FakeFs::new(cx.background_executor().clone());
        let language_registry = LanguageRegistry::new(cx.background_executor().clone());

        let language_registry = Arc::new(language_registry);
        languages::init(language_registry.clone(), fs, node_runtime, cx);

        if let Some(theme) = theme_settings.switch_theme("One Light", cx) {
            language_registry.set_theme(theme);
            ThemeSettings::override_global(theme_settings, cx);
        }

        let parsed_markdown = smol::block_on(async {
            parse_markdown(include_str!("./example.md"), None, Some(language_registry)).await
        });

        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1024.), px(800.0)),
                    cx,
                ))),
                ..Default::default()
            },
            |window, cx| {
                theme::setup_ui_font(window, cx);
                cx.new(|_| MarkdownPreviewExample { parsed_markdown })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
