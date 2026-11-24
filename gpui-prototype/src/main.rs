//! sshterm GPUI Kit port: phase 1, the shell layout (see `docs/gpui-port.md`).
mod console;
mod layout;
mod model;
mod panes;
mod workspace;

use std::path::PathBuf;

use gpui_kit::component::{
    TitleBar,
    theme::{Theme, ThemeRegistry},
};
use gpui_kit::{SharedString, px, size, *};

use workspace::Workspace;

fn main() {
    application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        init(cx);
        let default_theme: SharedString = "Tokyo Night".into();
        if let Err(error) = ThemeRegistry::watch_dir(PathBuf::from("gpui-prototype/themes"), cx, move |cx| {
            if let Some(theme) = ThemeRegistry::global(cx).themes().get(&default_theme).cloned() {
                Theme::update(cx, |current| current.apply_config(&theme));
            }
        }) {
            eprintln!("Failed to load bundled themes: {error}");
        }
        let mut options = TitleBar::window_options();
        options.window_min_size = Some(size(px(900.0), px(560.0))); // production minimum window size
        open_window(options, cx, |window, cx| cx.new(|cx| Workspace::new(window, cx)))
            .expect("open the sshterm GPUI window");
    });
}
