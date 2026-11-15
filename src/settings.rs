//! Persistent user preferences (the Unix counterpart of Paint's `CAttributeStore` registry
//! profile): per-tool sizes and opacities, colours, view toggles, text style, recent files.

use crate::brush::{BrushKind, Media};
use crate::color::Rgba;
use crate::shapes::ShapeKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TextStyle {
    pub family: String,
    pub size_pt: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// 0 = left, 1 = center, 2 = right
    pub align: u8,
    /// Fill the text box with the secondary colour.
    pub fill_background: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            family: String::new(),
            size_pt: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            align: 0,
            fill_background: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub primary: Rgba,
    pub secondary: Rgba,
    pub custom_colors: Vec<Rgba>,
    pub brush: BrushKind,
    pub shape: ShapeKind,
    pub sizes: HashMap<String, f32>,
    pub opacities: HashMap<String, f32>,
    pub outline: Media,
    pub fill: Media,
    pub shape_size: f32,
    pub transparent_selection: bool,
    pub free_form_select: bool,
    pub text: TextStyle,
    pub show_rulers: bool,
    pub show_grid: bool,
    pub show_status_bar: bool,
    pub show_layers: bool,
    pub theme: ThemePref,
    pub recent: Vec<PathBuf>,
    pub new_width: u32,
    pub new_height: u32,
    pub units: Units,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Units {
    #[default]
    Pixels,
    Inches,
    Centimeters,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            primary: Rgba::BLACK,
            secondary: Rgba::WHITE,
            custom_colors: Vec::new(),
            brush: BrushKind::Brush,
            shape: ShapeKind::Rectangle,
            sizes: HashMap::new(),
            opacities: HashMap::new(),
            outline: Media::Solid,
            fill: Media::None,
            shape_size: 3.0,
            transparent_selection: false,
            free_form_select: false,
            text: TextStyle::default(),
            show_rulers: false,
            show_grid: false,
            show_status_bar: true,
            show_layers: false,
            theme: ThemePref::System,
            recent: Vec::new(),
            new_width: crate::doc::DEFAULT_WIDTH,
            new_height: crate::doc::DEFAULT_HEIGHT,
            units: Units::Pixels,
        }
    }
}

fn key(k: BrushKind) -> String {
    format!("{k:?}")
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("unix-paint").join("settings.json"))
    }

    pub fn load() -> Self {
        let mut s: Settings = Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.sanitize();
        s
    }

    /// Bring hand-edited or stale values back into range so they can't break the app.
    pub fn sanitize(&mut self) {
        let max = crate::doc::MAX_DIM;
        self.new_width = self.new_width.clamp(1, max);
        self.new_height = self.new_height.clamp(1, max);
        self.shape_size = if self.shape_size.is_finite() { self.shape_size.clamp(1.0, 50.0) } else { 3.0 };
        self.text.size_pt = if self.text.size_pt.is_finite() { self.text.size_pt.clamp(1.0, 999.0) } else { 11.0 };
        self.text.align = self.text.align.min(2);
        self.primary.a = 255;
        self.secondary.a = 255;
        self.custom_colors.truncate(10);
        self.recent.truncate(10);
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                // Write-then-rename so a crash mid-write can't leave a truncated file.
                let tmp = p.with_extension("json.tmp");
                if std::fs::write(&tmp, s).is_ok() && std::fs::rename(&tmp, &p).is_err() {
                    let _ = std::fs::remove_file(&tmp);
                }
            }
        }
    }

    pub fn size(&self, k: BrushKind) -> f32 {
        let (lo, hi) = k.size_range();
        self.sizes.get(&key(k)).copied().unwrap_or(k.default_size()).clamp(lo, hi)
    }

    pub fn set_size(&mut self, k: BrushKind, v: f32) {
        let (lo, hi) = k.size_range();
        self.sizes.insert(key(k), v.round().clamp(lo, hi));
    }

    pub fn opacity(&self, k: BrushKind) -> f32 {
        self.opacities.get(&key(k)).copied().unwrap_or(k.default_opacity()).clamp(0.01, 1.0)
    }

    pub fn set_opacity(&mut self, k: BrushKind, v: f32) {
        self.opacities.insert(key(k), v.clamp(0.01, 1.0));
    }

    pub fn add_recent(&mut self, p: PathBuf) {
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }

    pub fn add_custom_color(&mut self, c: Rgba) {
        self.custom_colors.retain(|x| *x != c);
        self.custom_colors.insert(0, c);
        self.custom_colors.truncate(10);
    }
}
