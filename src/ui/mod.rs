pub mod canvas;
pub mod chrome;
pub mod dialogs;
pub mod shortcuts;
pub mod theme;
pub mod toolbar;

/// The active palette, stashed in egui memory by `theme::apply`.
pub fn palette_of(ui: &egui::Ui) -> theme::Palette {
    ui.ctx().data(|d| d.get_temp::<theme::Palette>(egui::Id::unique("palette"))).unwrap_or_else(|| theme::palette(ui.visuals().dark_mode))
}

/// Shortcut text as shown in menus: the app binds the platform's command key, so on macOS
/// "Ctrl+S" reads "⌘S".
pub fn shortcut_label(s: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(target_os = "macos") {
        s.replace("Alt+F4", "⌘Q").replace("Ctrl+Shift+", "⇧⌘").replace("Ctrl+", "⌘").into()
    } else {
        s.into()
    }
}
