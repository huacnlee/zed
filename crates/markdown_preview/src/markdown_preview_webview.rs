use std::{rc::Rc, sync::Arc, time::Duration};

use crate::{markdown_elements::ParsedMarkdown, markdown_parser::parse_markdown, OpenPreview};
use anyhow::Result;
use editor::{Editor, EditorEvent};
use gpui::{EventEmitter, FocusHandle, FocusableView, Render, Subscription, Task, View, WebView};
use language::LanguageRegistry;
use ui::{
    div, IntoElement, ParentElement as _, SharedString, Styled, ViewContext, VisualContext,
    WindowContext,
};
use workspace::{
    item::{Item, ItemHandle},
    Pane, Workspace,
};

const REPARSE_DEBOUNCE: Duration = Duration::from_millis(200);

pub struct MarkdownPreviewWebView {
    focus_handle: FocusHandle,
    view: Rc<wry::WebView>,
    active_editor: Option<EditorState>,
    tab_description: Option<String>,
    fallback_tab_description: SharedString,
    parsing_markdown_task: Option<Task<Result<()>>>,
    language_registry: Arc<LanguageRegistry>,
    contents: Option<ParsedMarkdown>,
}

struct EditorState {
    editor: View<Editor>,
    _subscription: Subscription,
}

impl MarkdownPreviewWebView {
    pub fn register(workspace: &mut Workspace, cx: &mut ViewContext<Workspace>) {
        workspace.register_action(move |workspace, _: &OpenPreview, cx| {
            if let Some(editor) = Self::resolve_active_item_as_markdown_editor(workspace, cx) {
                let view = Self::create_view(editor, workspace, cx);

                workspace.active_pane().update(cx, |pane, cx| {
                    if let Some(existing_view_idx) = Self::find_existing_preview_item_idx(pane) {
                        pane.activate_item(existing_view_idx, true, true, cx);
                    } else {
                        pane.add_item(Box::new(view.clone()), true, true, None, cx)
                    }
                });
                cx.notify();
            }
        });
    }

    fn create_view(
        editor: View<Editor>,
        workspace: &mut Workspace,
        cx: &mut ViewContext<Workspace>,
    ) -> View<MarkdownPreviewWebView> {
        let focus_handle = cx.focus_handle();
        let webview = wry::WebView::new_as_child(&cx.raw_window_handle()).unwrap();
        webview.load_url("https://google.com").unwrap();
        let language_registry = workspace.project().read(cx).languages().clone();

        let mut this = MarkdownPreviewWebView {
            focus_handle,
            view: Rc::new(webview),
            active_editor: None,
            tab_description: None,
            fallback_tab_description: "Markdown Preview".into(),
            parsing_markdown_task: None,
            language_registry,
            contents: None,
        };

        cx.new_view(|cx| {
            this.set_editor(editor, cx);
            this
        })
    }

    fn resolve_active_item_as_markdown_editor(
        workspace: &Workspace,
        cx: &mut ViewContext<Workspace>,
    ) -> Option<View<Editor>> {
        if let Some(editor) = workspace
            .active_item(cx)
            .and_then(|item| item.act_as::<Editor>(cx))
        {
            if Self::is_markdown_file(&editor, cx) {
                return Some(editor);
            }
        }
        None
    }

    fn find_existing_preview_item_idx(pane: &Pane) -> Option<usize> {
        pane.items_of_type::<MarkdownPreviewWebView>()
            .nth(0)
            .and_then(|view| pane.index_for_item(&view))
    }

    fn set_editor(&mut self, editor: View<Editor>, cx: &mut ViewContext<Self>) {
        if let Some(active) = &self.active_editor {
            if active.editor == editor {
                return;
            }
        }

        let subscription = cx.subscribe(&editor, |this, _, event: &EditorEvent, cx| {
            match event {
                EditorEvent::Edited { .. } => {
                    this.parse_markdown_from_active_editor(true, cx);
                }
                _ => {}
            };
        });

        self.tab_description = editor
            .read(cx)
            .tab_description(0, cx)
            .map(|tab_description| format!("Preview {}", tab_description));

        self.active_editor = Some(EditorState {
            editor,
            _subscription: subscription,
        });

        self.parse_markdown_from_active_editor(false, cx);
    }

    fn workspace_updated(
        &mut self,
        active_item: Option<Box<dyn ItemHandle>>,
        cx: &mut ViewContext<Self>,
    ) {
        if let Some(item) = active_item {
            if item.item_id() != cx.entity_id() {
                if let Some(editor) = item.act_as::<Editor>(cx) {
                    if Self::is_markdown_file(&editor, cx) {
                        self.set_editor(editor, cx);
                    }
                }
            }
        }
    }

    fn is_markdown_file<V>(editor: &View<Editor>, cx: &mut ViewContext<V>) -> bool {
        let language = editor.read(cx).buffer().read(cx).language_at(0, cx);
        language
            .map(|l| l.name().as_ref() == "Markdown")
            .unwrap_or(false)
    }

    fn parse_markdown_from_active_editor(
        &mut self,
        wait_for_debounce: bool,
        cx: &mut ViewContext<Self>,
    ) {
        if let Some(state) = &self.active_editor {
            self.parsing_markdown_task = Some(self.parse_markdown_in_background(
                wait_for_debounce,
                state.editor.clone(),
                cx,
            ));
        }
    }

    fn parse_markdown_in_background(
        &mut self,
        wait_for_debounce: bool,
        editor: View<Editor>,
        cx: &mut ViewContext<Self>,
    ) -> Task<Result<()>> {
        cx.spawn(move |view, mut cx| async move {
            if wait_for_debounce {
                // Wait for the user to stop typing
                cx.background_executor().timer(REPARSE_DEBOUNCE).await;
            }

            let contents = cx
                .update(|cx| editor.read(cx).buffer().read(cx).snapshot(cx).text())
                .unwrap();

            let parsing_task = cx.background_executor().spawn(async move {
                let parser = pulldown_cmark::Parser::new(&contents);
                let mut html_output = String::new();
                pulldown_cmark::html::push_html(&mut html_output, parser);
                html_output
            });

            let contents = parsing_task.await;
            view.update(&mut cx, move |view, cx| {
                let js = format!(r#"document.body.innerHTML = `<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/github-markdown-css/5.6.1/github-markdown.min.css" /><article class="markdown-body">{}</article>`;"#, contents);
                view.view.evaluate_script(&js).unwrap();
                cx.notify();
            })
        })
    }
}

impl Render for MarkdownPreviewWebView {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        div().size_full().child(WebView::new(self.view.clone()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewEvent {}

impl FocusableView for MarkdownPreviewWebView {
    fn focus_handle(&self, _cx: &gpui::AppContext) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PreviewEvent> for MarkdownPreviewWebView {}

impl Item for MarkdownPreviewWebView {
    type Event = PreviewEvent;

    fn tab_content(
        &self,
        _params: workspace::item::TabContentParams,
        _cx: &WindowContext,
    ) -> gpui::AnyElement {
        self.tab_description
            .clone()
            .map(|s| s.into_any_element())
            .unwrap_or(self.fallback_tab_description.clone().into_any_element())
    }

    fn show_toolbar(&self) -> bool {
        false
    }
}
