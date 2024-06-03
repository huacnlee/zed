use gpui::{Menu, MenuItem};
use std::borrow::Cow;

pub fn app_menus() -> Vec<Menu<'static>> {
    use crate::actions::Quit;

    vec![Menu {
        name: Cow::from("Storybook"),
        items: vec![MenuItem::action("Quit", Quit)],
    }]
}
