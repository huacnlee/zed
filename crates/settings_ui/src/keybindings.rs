use std::{ops::Range, sync::Arc};

use anyhow::{Context as _, anyhow};
use collections::HashSet;
use editor::{Editor, EditorEvent};
use feature_flags::FeatureFlagViewExt;
use fs::Fs;
use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    AppContext as _, AsyncApp, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    FontWeight, Global, KeyContext, Keystroke, ModifiersChangedEvent, ScrollStrategy, StyledText,
    Subscription, WeakEntity, actions, div,
};
use language::{Language, LanguageConfig};
use settings::KeybindSource;

use util::ResultExt;

use ui::{
    ActiveTheme as _, App, BorrowAppContext, ParentElement as _, Render, SharedString, Styled as _,
    Window, prelude::*,
};
use workspace::{Item, ModalView, SerializableItem, Workspace, register_serializable_item};

use crate::{
    SettingsUiFeatureFlag,
    keybindings::persistence::KEYBINDING_EDITORS,
    ui_components::table::{Table, TableInteractionState},
};

actions!(zed, [OpenKeymapEditor]);

pub fn init(cx: &mut App) {
    let keymap_event_channel = KeymapEventChannel::new();
    cx.set_global(keymap_event_channel);

    cx.on_action(|_: &OpenKeymapEditor, cx| {
        workspace::with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let existing = workspace
                .active_pane()
                .read(cx)
                .items()
                .find_map(|item| item.downcast::<KeymapEditor>());

            if let Some(existing) = existing {
                workspace.activate_item(&existing, true, true, window, cx);
            } else {
                let keymap_editor =
                    cx.new(|cx| KeymapEditor::new(workspace.weak_handle(), window, cx));
                workspace.add_item_to_active_pane(Box::new(keymap_editor), None, true, window, cx);
            }
        });
    });

    cx.observe_new(|_workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };

        let keymap_ui_actions = [std::any::TypeId::of::<OpenKeymapEditor>()];

        command_palette_hooks::CommandPaletteFilter::update_global(cx, |filter, _cx| {
            filter.hide_action_types(&keymap_ui_actions);
        });

        cx.observe_flag::<SettingsUiFeatureFlag, _>(
            window,
            move |is_enabled, _workspace, _, cx| {
                if is_enabled {
                    command_palette_hooks::CommandPaletteFilter::update_global(
                        cx,
                        |filter, _cx| {
                            filter.show_action_types(keymap_ui_actions.iter());
                        },
                    );
                } else {
                    command_palette_hooks::CommandPaletteFilter::update_global(
                        cx,
                        |filter, _cx| {
                            filter.hide_action_types(&keymap_ui_actions);
                        },
                    );
                }
            },
        )
        .detach();
    })
    .detach();

    register_serializable_item::<KeymapEditor>(cx);
}

pub struct KeymapEventChannel {}

impl Global for KeymapEventChannel {}

impl KeymapEventChannel {
    fn new() -> Self {
        Self {}
    }

    pub fn trigger_keymap_changed(cx: &mut App) {
        let Some(_event_channel) = cx.try_global::<Self>() else {
            // don't panic if no global defined. This usually happens in tests
            return;
        };
        cx.update_global(|_event_channel: &mut Self, _| {
            /* triggers observers in KeymapEditors */
        });
    }
}

struct KeymapEditor {
    workspace: WeakEntity<Workspace>,
    focus_handle: FocusHandle,
    _keymap_subscription: Subscription,
    keybindings: Vec<ProcessedKeybinding>,
    // corresponds 1 to 1 with keybindings
    string_match_candidates: Arc<Vec<StringMatchCandidate>>,
    matches: Vec<StringMatch>,
    table_interaction_state: Entity<TableInteractionState>,
    filter_editor: Entity<Editor>,
    selected_index: Option<usize>,
}

impl EventEmitter<()> for KeymapEditor {}

impl Focusable for KeymapEditor {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        return self.filter_editor.focus_handle(cx);
    }
}

impl KeymapEditor {
    fn new(workspace: WeakEntity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();

        let _keymap_subscription =
            cx.observe_global::<KeymapEventChannel>(Self::update_keybindings);
        let table_interaction_state = TableInteractionState::new(window, cx);

        let filter_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Filter action names...", cx);
            editor
        });

        cx.subscribe(&filter_editor, |this, _, e: &EditorEvent, cx| {
            if !matches!(e, EditorEvent::BufferEdited) {
                return;
            }

            this.update_matches(cx);
        })
        .detach();

        let mut this = Self {
            workspace,
            keybindings: vec![],
            string_match_candidates: Arc::new(vec![]),
            matches: vec![],
            focus_handle: focus_handle.clone(),
            _keymap_subscription,
            table_interaction_state,
            filter_editor,
            selected_index: None,
        };

        this.update_keybindings(cx);

        this
    }

    fn current_query(&self, cx: &mut Context<Self>) -> String {
        self.filter_editor.read(cx).text(cx)
    }

    fn update_matches(&self, cx: &mut Context<Self>) {
        let query = self.current_query(cx);

        cx.spawn(async move |this, cx| Self::process_query(this, query, cx).await)
            .detach();
    }

    async fn process_query(
        this: WeakEntity<Self>,
        query: String,
        cx: &mut AsyncApp,
    ) -> Result<(), db::anyhow::Error> {
        let query = command_palette::normalize_action_query(&query);
        let (string_match_candidates, keybind_count) = this.read_with(cx, |this, _| {
            (this.string_match_candidates.clone(), this.keybindings.len())
        })?;
        let executor = cx.background_executor().clone();
        let matches = fuzzy::match_strings(
            &string_match_candidates,
            &query,
            true,
            true,
            keybind_count,
            &Default::default(),
            executor,
        )
        .await;
        this.update(cx, |this, cx| {
            this.selected_index.take();
            this.scroll_to_item(0, ScrollStrategy::Top, cx);
            this.matches = matches;
            cx.notify();
        })
    }

    fn process_bindings(
        json_language: Arc<Language>,
        cx: &mut Context<Self>,
    ) -> (Vec<ProcessedKeybinding>, Vec<StringMatchCandidate>) {
        let key_bindings_ptr = cx.key_bindings();
        let lock = key_bindings_ptr.borrow();
        let key_bindings = lock.bindings();
        let mut unmapped_action_names = HashSet::from_iter(cx.all_action_names());

        let mut processed_bindings = Vec::new();
        let mut string_match_candidates = Vec::new();

        for key_binding in key_bindings {
            let source = key_binding.meta().map(settings::KeybindSource::from_meta);

            let keystroke_text = ui::text_for_keystrokes(key_binding.keystrokes(), cx);
            let ui_key_binding = Some(
                ui::KeyBinding::new(key_binding.clone(), cx)
                    .vim_mode(source == Some(settings::KeybindSource::Vim)),
            );

            let context = key_binding
                .predicate()
                .map(|predicate| predicate.to_string())
                .unwrap_or_else(|| "<global>".to_string());

            let source = source.map(|source| (source, source.name().into()));

            let action_name = key_binding.action().name();
            unmapped_action_names.remove(&action_name);
            let action_input = key_binding
                .action_input()
                .map(|input| TextWithSyntaxHighlighting::new(input, json_language.clone()));

            let index = processed_bindings.len();
            let string_match_candidate = StringMatchCandidate::new(index, &action_name);
            processed_bindings.push(ProcessedKeybinding {
                keystroke_text: keystroke_text.into(),
                ui_key_binding,
                action: action_name.into(),
                action_input,
                context: context.into(),
                source,
            });
            string_match_candidates.push(string_match_candidate);
        }

        let empty = SharedString::new_static("");
        for action_name in unmapped_action_names.into_iter() {
            let index = processed_bindings.len();
            let string_match_candidate = StringMatchCandidate::new(index, &action_name);
            processed_bindings.push(ProcessedKeybinding {
                keystroke_text: empty.clone(),
                ui_key_binding: None,
                action: (*action_name).into(),
                action_input: None,
                context: empty.clone(),
                source: None,
            });
            string_match_candidates.push(string_match_candidate);
        }

        (processed_bindings, string_match_candidates)
    }

    fn update_keybindings(&mut self, cx: &mut Context<KeymapEditor>) {
        let workspace = self.workspace.clone();
        cx.spawn(async move |this, cx| {
            let json_language = Self::load_json_language(workspace, cx).await;
            let query = this.update(cx, |this, cx| {
                let (key_bindings, string_match_candidates) =
                    Self::process_bindings(json_language.clone(), cx);
                this.keybindings = key_bindings;
                this.string_match_candidates = Arc::new(string_match_candidates);
                this.matches = this
                    .string_match_candidates
                    .iter()
                    .enumerate()
                    .map(|(ix, candidate)| StringMatch {
                        candidate_id: ix,
                        score: 0.0,
                        positions: vec![],
                        string: candidate.string.clone(),
                    })
                    .collect();
                this.current_query(cx)
            })?;
            // calls cx.notify
            Self::process_query(this, query, cx).await
        })
        .detach_and_log_err(cx);
    }

    async fn load_json_language(
        workspace: WeakEntity<Workspace>,
        cx: &mut AsyncApp,
    ) -> Arc<Language> {
        let json_language_task = workspace
            .read_with(cx, |workspace, cx| {
                workspace
                    .project()
                    .read(cx)
                    .languages()
                    .language_for_name("JSON")
            })
            .context("Failed to load JSON language")
            .log_err();
        let json_language = match json_language_task {
            Some(task) => task.await.context("Failed to load JSON language").log_err(),
            None => None,
        };
        return json_language.unwrap_or_else(|| {
            Arc::new(Language::new(
                LanguageConfig {
                    name: "JSON".into(),
                    ..Default::default()
                },
                Some(tree_sitter_json::LANGUAGE.into()),
            ))
        });
    }

    fn dispatch_context(&self, _window: &Window, _cx: &Context<Self>) -> KeyContext {
        let mut dispatch_context = KeyContext::new_with_defaults();
        dispatch_context.add("KeymapEditor");
        dispatch_context.add("menu");

        dispatch_context
    }

    fn scroll_to_item(&self, index: usize, strategy: ScrollStrategy, cx: &mut App) {
        let index = usize::min(index, self.matches.len().saturating_sub(1));
        self.table_interaction_state.update(cx, |this, _cx| {
            this.scroll_handle.scroll_to_item(index, strategy);
        });
    }

    fn select_next(&mut self, _: &menu::SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(selected) = self.selected_index {
            let selected = selected + 1;
            if selected >= self.matches.len() {
                self.select_last(&Default::default(), window, cx);
            } else {
                self.selected_index = Some(selected);
                self.scroll_to_item(selected, ScrollStrategy::Center, cx);
                cx.notify();
            }
        } else {
            self.select_first(&Default::default(), window, cx);
        }
    }

    fn select_previous(
        &mut self,
        _: &menu::SelectPrevious,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(selected) = self.selected_index {
            if selected == 0 {
                return;
            }

            let selected = selected - 1;

            if selected >= self.matches.len() {
                self.select_last(&Default::default(), window, cx);
            } else {
                self.selected_index = Some(selected);
                self.scroll_to_item(selected, ScrollStrategy::Center, cx);
                cx.notify();
            }
        } else {
            self.select_last(&Default::default(), window, cx);
        }
    }

    fn select_first(
        &mut self,
        _: &menu::SelectFirst,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.matches.get(0).is_some() {
            self.selected_index = Some(0);
            self.scroll_to_item(0, ScrollStrategy::Center, cx);
            cx.notify();
        }
    }

    fn select_last(&mut self, _: &menu::SelectLast, _window: &mut Window, cx: &mut Context<Self>) {
        if self.matches.last().is_some() {
            let index = self.matches.len() - 1;
            self.selected_index = Some(index);
            self.scroll_to_item(index, ScrollStrategy::Center, cx);
            cx.notify();
        }
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected_index else {
            return;
        };
        let keybind = self.keybindings[self.matches[index].candidate_id].clone();

        self.edit_keybinding(keybind, window, cx);
    }

    fn edit_keybinding(
        &mut self,
        keybind: ProcessedKeybinding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| {
                let fs = workspace.app_state().fs.clone();
                workspace.toggle_modal(window, cx, |window, cx| {
                    let modal = KeybindingEditorModal::new(keybind, fs, window, cx);
                    window.focus(&modal.focus_handle(cx));
                    modal
                });
            })
            .log_err();
    }

    fn focus_search(
        &mut self,
        _: &search::FocusSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self
            .filter_editor
            .focus_handle(cx)
            .contains_focused(window, cx)
        {
            window.focus(&self.filter_editor.focus_handle(cx));
        } else {
            self.filter_editor.update(cx, |editor, cx| {
                editor.select_all(&Default::default(), window, cx);
            });
        }
        self.selected_index.take();
    }
}

#[derive(Clone)]
struct ProcessedKeybinding {
    keystroke_text: SharedString,
    ui_key_binding: Option<ui::KeyBinding>,
    action: SharedString,
    action_input: Option<TextWithSyntaxHighlighting>,
    context: SharedString,
    source: Option<(KeybindSource, SharedString)>,
}

impl Item for KeymapEditor {
    type Event = ();

    fn tab_content_text(&self, _detail: usize, _cx: &App) -> ui::SharedString {
        "Keymap Editor".into()
    }
}

impl Render for KeymapEditor {
    fn render(&mut self, window: &mut Window, cx: &mut ui::Context<Self>) -> impl ui::IntoElement {
        let row_count = self.matches.len();
        let theme = cx.theme();

        div()
            .key_context(self.dispatch_context(window, cx))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_first))
            .on_action(cx.listener(Self::select_last))
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::confirm))
            .size_full()
            .bg(theme.colors().editor_background)
            .id("keymap-editor")
            .track_focus(&self.focus_handle)
            .px_4()
            .v_flex()
            .pb_4()
            .child(
                h_flex()
                    .key_context({
                        let mut context = KeyContext::new_with_defaults();
                        context.add("BufferSearchBar");
                        context
                    })
                    .w_full()
                    .h_12()
                    .px_4()
                    .my_4()
                    .border_2()
                    .border_color(theme.colors().border)
                    .child(self.filter_editor.clone()),
            )
            .child(
                Table::new()
                    .interactable(&self.table_interaction_state)
                    .striped()
                    .column_widths([rems(16.), rems(16.), rems(16.), rems(32.), rems(8.)])
                    .header(["Action", "Arguments", "Keystrokes", "Context", "Source"])
                    .selected_item_index(self.selected_index)
                    .on_click_row(cx.processor(|this, row_index, _window, _cx| {
                        this.selected_index = Some(row_index);
                    }))
                    .uniform_list(
                        "keymap-editor-table",
                        row_count,
                        cx.processor(move |this, range: Range<usize>, _window, _cx| {
                            range
                                .filter_map(|index| {
                                    let candidate_id = this.matches.get(index)?.candidate_id;
                                    let binding = &this.keybindings[candidate_id];

                                    let action = binding.action.clone().into_any_element();
                                    let keystrokes = binding.ui_key_binding.clone().map_or(
                                        binding.keystroke_text.clone().into_any_element(),
                                        IntoElement::into_any_element,
                                    );
                                    let action_input = binding
                                        .action_input
                                        .clone()
                                        .map_or(gpui::Empty.into_any_element(), |input| {
                                            input.into_any_element()
                                        });
                                    let context = binding.context.clone().into_any_element();
                                    let source = binding
                                        .source
                                        .clone()
                                        .map(|(_source, name)| name)
                                        .unwrap_or_default()
                                        .into_any_element();
                                    Some([action, action_input, keystrokes, context, source])
                                })
                                .collect()
                        }),
                    ),
            )
    }
}

#[derive(Debug, Clone, IntoElement)]
struct TextWithSyntaxHighlighting {
    text: SharedString,
    language: Arc<Language>,
}

impl TextWithSyntaxHighlighting {
    pub fn new(text: impl Into<SharedString>, language: Arc<Language>) -> Self {
        Self {
            text: text.into(),
            language,
        }
    }
}

impl RenderOnce for TextWithSyntaxHighlighting {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let text_style = window.text_style();
        let syntax_theme = cx.theme().syntax();

        let text = self.text.clone();

        let highlights = self
            .language
            .highlight_text(&text.as_ref().into(), 0..text.len());
        let mut runs = Vec::with_capacity(highlights.len());
        let mut offset = 0;

        for (highlight_range, highlight_id) in highlights {
            // Add un-highlighted text before the current highlight
            if highlight_range.start > offset {
                runs.push(text_style.to_run(highlight_range.start - offset));
            }

            let mut run_style = text_style.clone();
            if let Some(highlight_style) = highlight_id.style(syntax_theme) {
                run_style = run_style.highlight(highlight_style);
            }
            // add the highlighted range
            runs.push(run_style.to_run(highlight_range.len()));
            offset = highlight_range.end;
        }

        // Add any remaining un-highlighted text
        if offset < text.len() {
            runs.push(text_style.to_run(text.len() - offset));
        }

        return StyledText::new(text).with_runs(runs);
    }
}

struct KeybindingEditorModal {
    editing_keybind: ProcessedKeybinding,
    keybind_editor: Entity<KeybindInput>,
    fs: Arc<dyn Fs>,
    error: Option<String>,
}

impl ModalView for KeybindingEditorModal {}

impl EventEmitter<DismissEvent> for KeybindingEditorModal {}

impl Focusable for KeybindingEditorModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.keybind_editor.focus_handle(cx)
    }
}

impl KeybindingEditorModal {
    pub fn new(
        editing_keybind: ProcessedKeybinding,
        fs: Arc<dyn Fs>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let keybind_editor = cx.new(KeybindInput::new);
        Self {
            editing_keybind,
            fs,
            keybind_editor,
            error: None,
        }
    }
}

impl Render for KeybindingEditorModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().colors();
        return v_flex()
            .gap_4()
            .w(rems(36.))
            .child(
                v_flex()
                    .items_center()
                    .text_center()
                    .bg(theme.background)
                    .border_color(theme.border)
                    .border_2()
                    .px_4()
                    .py_2()
                    .w_full()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("Input desired keybinding, then hit save"),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .child(self.keybind_editor.clone())
                            .child(
                                IconButton::new("backspace-btn", ui::IconName::Backspace).on_click(
                                    cx.listener(|this, _event, _window, cx| {
                                        this.keybind_editor.update(cx, |editor, cx| {
                                            editor.keystrokes.pop();
                                            cx.notify();
                                        })
                                    }),
                                ),
                            )
                            .child(IconButton::new("clear-btn", ui::IconName::Eraser).on_click(
                                cx.listener(|this, _event, _window, cx| {
                                    this.keybind_editor.update(cx, |editor, cx| {
                                        editor.keystrokes.clear();
                                        cx.notify();
                                    })
                                }),
                            )),
                    )
                    .child(
                        h_flex().w_full().items_center().justify_center().child(
                            Button::new("save-btn", "Save")
                                .label_size(LabelSize::Large)
                                .on_click(cx.listener(|this, _event, _window, cx| {
                                    let existing_keybind = this.editing_keybind.clone();
                                    let fs = this.fs.clone();
                                    let new_keystrokes = this
                                        .keybind_editor
                                        .read_with(cx, |editor, _| editor.keystrokes.clone());
                                    if new_keystrokes.is_empty() {
                                        this.error = Some("Keystrokes cannot be empty".to_string());
                                        cx.notify();
                                        return;
                                    }
                                    let tab_size =
                                        cx.global::<settings::SettingsStore>().json_tab_size();
                                    cx.spawn(async move |this, cx| {
                                        if let Err(err) = save_keybinding_update(
                                            existing_keybind,
                                            &new_keystrokes,
                                            &fs,
                                            tab_size,
                                        )
                                        .await
                                        {
                                            this.update(cx, |this, cx| {
                                                this.error = Some(err);
                                                cx.notify();
                                            })
                                            .log_err();
                                        }
                                    })
                                    .detach();
                                })),
                        ),
                    ),
            )
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    div()
                        .bg(theme.background)
                        .border_color(theme.border)
                        .border_2()
                        .rounded_md()
                        .child(error),
                )
            });
    }
}

async fn save_keybinding_update(
    existing: ProcessedKeybinding,
    new_keystrokes: &[Keystroke],
    fs: &Arc<dyn Fs>,
    tab_size: usize,
) -> Result<(), String> {
    let keymap_contents = settings::KeymapFile::load_keymap_file(fs)
        .await
        .map_err(|err| format!("Failed to load keymap file: {}", err))?;
    let existing_keystrokes = existing
        .ui_key_binding
        .as_ref()
        .map(|keybinding| keybinding.key_binding.keystrokes())
        .unwrap_or_default();
    let operation = if existing.ui_key_binding.is_some() {
        settings::KeybindUpdateOperation::Replace {
            target: settings::KeybindUpdateTarget {
                context: Some(existing.context.as_ref()).filter(|context| !context.is_empty()),
                keystrokes: existing_keystrokes,
                action_name: &existing.action,
                use_key_equivalents: false,
                input: existing
                    .action_input
                    .as_ref()
                    .map(|input| input.text.as_ref()),
            },
            target_source: existing
                .source
                .map(|(source, _name)| source)
                .unwrap_or(KeybindSource::User),
            source: settings::KeybindUpdateTarget {
                context: Some(existing.context.as_ref()).filter(|context| !context.is_empty()),
                keystrokes: new_keystrokes,
                action_name: &existing.action,
                use_key_equivalents: false,
                input: existing
                    .action_input
                    .as_ref()
                    .map(|input| input.text.as_ref()),
            },
        }
    } else {
        return Err(
            "Not Implemented: Creating new bindings from unbound actions is not supported yet."
                .to_string(),
        );
    };
    let updated_keymap_contents =
        settings::KeymapFile::update_keybinding(operation, keymap_contents, tab_size)
            .map_err(|err| format!("Failed to update keybinding: {}", err))?;
    fs.atomic_write(paths::keymap_file().clone(), updated_keymap_contents)
        .await
        .map_err(|err| format!("Failed to write keymap file: {}", err))?;
    Ok(())
}

struct KeybindInput {
    keystrokes: Vec<Keystroke>,
    focus_handle: FocusHandle,
}

impl KeybindInput {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            keystrokes: Vec::new(),
            focus_handle,
        }
    }

    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(last) = self.keystrokes.last_mut()
            && last.key.is_empty()
        {
            if !event.modifiers.modified() {
                self.keystrokes.pop();
            } else {
                last.modifiers = event.modifiers;
            }
        } else {
            self.keystrokes.push(Keystroke {
                modifiers: event.modifiers,
                key: "".to_string(),
                key_char: None,
            });
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn on_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.is_held {
            return;
        }
        if let Some(last) = self.keystrokes.last_mut()
            && last.key.is_empty()
        {
            *last = event.keystroke.clone();
        } else {
            self.keystrokes.push(event.keystroke.clone());
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn on_key_up(
        &mut self,
        event: &gpui::KeyUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(last) = self.keystrokes.last_mut()
            && !last.key.is_empty()
            && last.modifiers == event.keystroke.modifiers
        {
            self.keystrokes.push(Keystroke {
                modifiers: event.keystroke.modifiers,
                key: "".to_string(),
                key_char: None,
            });
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for KeybindInput {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for KeybindInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors();
        return div()
            .track_focus(&self.focus_handle)
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .focus(|mut style| {
                style.border_color = Some(colors.border_focused);
                style
            })
            .h_12()
            .w_full()
            .bg(colors.editor_background)
            .border_2()
            .border_color(colors.border)
            .p_4()
            .flex_row()
            .text_center()
            .justify_center()
            .child(ui::text_for_keystrokes(&self.keystrokes, cx));
    }
}

impl SerializableItem for KeymapEditor {
    fn serialized_item_kind() -> &'static str {
        "KeymapEditor"
    }

    fn cleanup(
        workspace_id: workspace::WorkspaceId,
        alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        cx: &mut App,
    ) -> gpui::Task<gpui::Result<()>> {
        workspace::delete_unloaded_items(
            alive_items,
            workspace_id,
            "keybinding_editors",
            &KEYBINDING_EDITORS,
            cx,
        )
    }

    fn deserialize(
        _project: Entity<project::Project>,
        workspace: WeakEntity<Workspace>,
        workspace_id: workspace::WorkspaceId,
        item_id: workspace::ItemId,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui::Task<gpui::Result<Entity<Self>>> {
        window.spawn(cx, async move |cx| {
            if KEYBINDING_EDITORS
                .get_keybinding_editor(item_id, workspace_id)?
                .is_some()
            {
                cx.update(|window, cx| cx.new(|cx| KeymapEditor::new(workspace, window, cx)))
            } else {
                Err(anyhow!("No keybinding editor to deserialize"))
            }
        })
    }

    fn serialize(
        &mut self,
        workspace: &mut Workspace,
        item_id: workspace::ItemId,
        _closing: bool,
        _window: &mut Window,
        cx: &mut ui::Context<Self>,
    ) -> Option<gpui::Task<gpui::Result<()>>> {
        let workspace_id = workspace.database_id()?;
        Some(cx.background_spawn(async move {
            KEYBINDING_EDITORS
                .save_keybinding_editor(item_id, workspace_id)
                .await
        }))
    }

    fn should_serialize(&self, _event: &Self::Event) -> bool {
        false
    }
}

mod persistence {
    use db::{define_connection, query, sqlez_macros::sql};
    use workspace::WorkspaceDb;

    define_connection! {
        pub static ref KEYBINDING_EDITORS: KeybindingEditorDb<WorkspaceDb> =
            &[sql!(
                CREATE TABLE keybinding_editors (
                    workspace_id INTEGER,
                    item_id INTEGER UNIQUE,

                    PRIMARY KEY(workspace_id, item_id),
                    FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                    ON DELETE CASCADE
                ) STRICT;
            )];
    }

    impl KeybindingEditorDb {
        query! {
            pub async fn save_keybinding_editor(
                item_id: workspace::ItemId,
                workspace_id: workspace::WorkspaceId
            ) -> Result<()> {
                INSERT OR REPLACE INTO keybinding_editors(item_id, workspace_id)
                VALUES (?, ?)
            }
        }

        query! {
            pub fn get_keybinding_editor(
                item_id: workspace::ItemId,
                workspace_id: workspace::WorkspaceId
            ) -> Result<Option<workspace::ItemId>> {
                SELECT item_id
                FROM keybinding_editors
                WHERE item_id = ? AND workspace_id = ?
            }
        }
    }
}
