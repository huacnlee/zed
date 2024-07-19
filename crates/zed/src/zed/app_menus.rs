use collab_ui::collab_panel;
use gpui::{Menu, MenuItem, OsAction};
use i18n::t;
use terminal_view::terminal_panel;

pub fn app_menus() -> Vec<Menu> {
    use zed_actions::Quit;

    vec![
        Menu {
            name: "Zed".into(),
            items: vec![
                MenuItem::action(t!("About Zed…"), zed_actions::About),
                MenuItem::action(t!("Check for Updates"), auto_update::Check),
                MenuItem::separator(),
                MenuItem::submenu(Menu {
                    name: t!("Preferences"),
                    items: vec![
                        MenuItem::action(t!("Open Settings"), super::OpenSettings),
                        MenuItem::action(t!("Open Key Bindings"), zed_actions::OpenKeymap),
                        MenuItem::action(t!("Open Default Settings"), super::OpenDefaultSettings),
                        MenuItem::action(t!("Open Default Key Bindings"), super::OpenDefaultKeymap),
                        MenuItem::action(t!("Open Local Settings"), super::OpenLocalSettings),
                        MenuItem::action(t!("Select Theme…"), theme_selector::Toggle::default()),
                    ],
                }),
                MenuItem::action(t!("Extensions"), extensions_ui::Extensions),
                MenuItem::action(t!("Install CLI"), install_cli::Install),
                MenuItem::separator(),
                MenuItem::action(t!("Hide Zed"), super::Hide),
                MenuItem::action(t!("Hide Others"), super::HideOthers),
                MenuItem::action(t!("Show All"), super::ShowAll),
                MenuItem::action(t!("Quit"), Quit),
            ],
        },
        Menu {
            name: t!("File"),
            items: vec![
                MenuItem::action(t!("New"), workspace::NewFile),
                MenuItem::action(t!("New Window"), workspace::NewWindow),
                MenuItem::separator(),
                MenuItem::action(t!("Open…"), workspace::Open),
                MenuItem::action(
                    t!("Open Recent…"),
                    recent_projects::OpenRecent {
                        create_new_window: true,
                    },
                ),
                MenuItem::separator(),
                MenuItem::action(t!("Add Folder to Project…"), workspace::AddFolderToProject),
                MenuItem::action(t!("Save"), workspace::Save { save_intent: None }),
                MenuItem::action(t!("Save As…"), workspace::SaveAs),
                MenuItem::action(t!("Save All"), workspace::SaveAll { save_intent: None }),
                MenuItem::action(
                    t!("Close Editor"),
                    workspace::CloseActiveItem { save_intent: None },
                ),
                MenuItem::action(t!("Close Window"), workspace::CloseWindow),
            ],
        },
        Menu {
            name: t!("Edit"),
            items: vec![
                MenuItem::os_action(t!("Undo"), editor::actions::Undo, OsAction::Undo),
                MenuItem::os_action(t!("Redo"), editor::actions::Redo, OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action(t!("Cut"), editor::actions::Cut, OsAction::Cut),
                MenuItem::os_action(t!("Copy"), editor::actions::Copy, OsAction::Copy),
                MenuItem::os_action(t!("Paste"), editor::actions::Paste, OsAction::Paste),
                MenuItem::separator(),
                MenuItem::action(t!("Find"), search::buffer_search::Deploy::find()),
                MenuItem::action(t!("Find In Project"), workspace::DeploySearch::find()),
                MenuItem::separator(),
                MenuItem::action(
                    t!("Toggle Line Comment"),
                    editor::actions::ToggleComments::default(),
                ),
            ],
        },
        Menu {
            name: t!("Selection"),
            items: vec![
                MenuItem::os_action(
                    t!("Select All"),
                    editor::actions::SelectAll,
                    OsAction::SelectAll,
                ),
                MenuItem::action(
                    t!("Expand Selection"),
                    editor::actions::SelectLargerSyntaxNode,
                ),
                MenuItem::action(
                    t!("Shrink Selection"),
                    editor::actions::SelectSmallerSyntaxNode,
                ),
                MenuItem::separator(),
                MenuItem::action(t!("Add Cursor Above"), editor::actions::AddSelectionAbove),
                MenuItem::action(t!("Add Cursor Below"), editor::actions::AddSelectionBelow),
                MenuItem::action(
                    t!("Select Next Occurrence"),
                    editor::actions::SelectNext {
                        replace_newest: false,
                    },
                ),
                MenuItem::separator(),
                MenuItem::action(t!("Move Line Up"), editor::actions::MoveLineUp),
                MenuItem::action(t!("Move Line Down"), editor::actions::MoveLineDown),
                MenuItem::action(
                    t!("Duplicate Selection"),
                    editor::actions::DuplicateLineDown,
                ),
            ],
        },
        Menu {
            name: t!("View"),
            items: vec![
                MenuItem::action(t!("Zoom In"), zed_actions::IncreaseBufferFontSize),
                MenuItem::action(t!("Zoom Out"), zed_actions::DecreaseBufferFontSize),
                MenuItem::action(t!("Reset Zoom"), zed_actions::ResetBufferFontSize),
                MenuItem::separator(),
                MenuItem::action(t!("Toggle Left Dock"), workspace::ToggleLeftDock),
                MenuItem::action(t!("Toggle Right Dock"), workspace::ToggleRightDock),
                MenuItem::action(t!("Toggle Bottom Dock"), workspace::ToggleBottomDock),
                MenuItem::action(t!("Close All Docks"), workspace::CloseAllDocks),
                MenuItem::submenu(Menu {
                    name: t!("Editor Layout"),
                    items: vec![
                        MenuItem::action(t!("Split Up"), workspace::SplitUp),
                        MenuItem::action(t!("Split Down"), workspace::SplitDown),
                        MenuItem::action(t!("Split Left"), workspace::SplitLeft),
                        MenuItem::action(t!("Split Right"), workspace::SplitRight),
                    ],
                }),
                MenuItem::separator(),
                MenuItem::action(t!("Project Panel"), project_panel::ToggleFocus),
                MenuItem::action(t!("Outline Panel"), outline_panel::ToggleFocus),
                MenuItem::action(t!("Collab Panel"), collab_panel::ToggleFocus),
                MenuItem::action(t!("Terminal Panel"), terminal_panel::ToggleFocus),
                MenuItem::separator(),
                MenuItem::action(t!("Diagnostics"), diagnostics::Deploy),
                MenuItem::separator(),
            ],
        },
        Menu {
            name: t!("Go"),
            items: vec![
                MenuItem::action(t!("Back"), workspace::GoBack),
                MenuItem::action(t!("Forward"), workspace::GoForward),
                MenuItem::separator(),
                MenuItem::action(t!("Command Palette…"), command_palette::Toggle),
                MenuItem::separator(),
                MenuItem::action(t!("Go to File…"), workspace::ToggleFileFinder::default()),
                // MenuItem::action("Go to Symbol in Project", project_symbols::Toggle),
                MenuItem::action(
                    t!("Go to Symbol in Editor…"),
                    editor::actions::ToggleOutline,
                ),
                MenuItem::action(t!("Go to Line/Column…"), editor::actions::ToggleGoToLine),
                MenuItem::separator(),
                MenuItem::action(t!("Go to Definition"), editor::actions::GoToDefinition),
                MenuItem::action(
                    t!("Go to Type Definition"),
                    editor::actions::GoToTypeDefinition,
                ),
                MenuItem::action(
                    t!("Find All References"),
                    editor::actions::FindAllReferences,
                ),
                MenuItem::separator(),
                MenuItem::action(t!("Next Problem"), editor::actions::GoToDiagnostic),
                MenuItem::action(t!("Previous Problem"), editor::actions::GoToPrevDiagnostic),
            ],
        },
        Menu {
            name: t!("Window"),
            items: vec![
                MenuItem::action(t!("Minimize"), super::Minimize),
                MenuItem::action(t!("Zoom"), super::Zoom),
                MenuItem::separator(),
            ],
        },
        Menu {
            name: t!("Help"),
            items: vec![
                MenuItem::action(t!("View Telemetry"), zed_actions::OpenTelemetryLog),
                MenuItem::action(t!("View Dependency Licenses"), zed_actions::OpenLicenses),
                MenuItem::action(t!("Show Welcome"), workspace::Welcome),
                MenuItem::action(t!("Give Feedback..."), feedback::GiveFeedback),
                MenuItem::separator(),
                MenuItem::action(
                    t!("Documentation"),
                    super::OpenBrowser {
                        url: "https://zed.dev/docs".into(),
                    },
                ),
                MenuItem::action(
                    "Zed Twitter",
                    super::OpenBrowser {
                        url: "https://twitter.com/zeddotdev".into(),
                    },
                ),
                MenuItem::action(
                    "Join the Team",
                    super::OpenBrowser {
                        url: "https://zed.dev/jobs".into(),
                    },
                ),
            ],
        },
    ]
}
