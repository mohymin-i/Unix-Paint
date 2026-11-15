//! Paint for Unix — a rebuild of Windows Paint for Linux and macOS.

mod app;
mod brush;
mod color;
mod doc;
mod history;
mod icons;
mod io;
mod platform;
mod raster;
mod script;
mod selection;
mod settings;
mod shape_edit;
mod shapes;
mod text;
mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::init();
    let mut file: Option<PathBuf> = None;
    let mut screenshot: Option<(PathBuf, u32)> = None;
    let mut script: Option<script::Script> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            // Hidden: render a few frames, save a screenshot, exit (used for testing).
            "--screenshot" => screenshot = args.next().map(|p| (PathBuf::from(p), 30)),
            // Hidden: replay a UI script (see src/script.rs).
            "--script" => {
                let p = PathBuf::from(args.next().unwrap_or_default());
                match script::Script::load(&p) {
                    Ok(s) => script = Some(s),
                    Err(e) => {
                        eprintln!("can't read script {}: {e}", p.display());
                        std::process::exit(2);
                    }
                }
            }
            // Hidden: write the generated app icon (used by packaging scripts).
            "--export-icon" => {
                let p = PathBuf::from(args.next().unwrap_or_else(|| "unix-paint.png".into()));
                let icon = app_icon_sized(256);
                let img = image::RgbaImage::from_raw(icon.width, icon.height, icon.rgba).expect("icon");
                if let Err(e) = img.save(&p) {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
                return Ok(());
            }
            "-h" | "--help" => {
                println!("Usage: unix-paint [FILE]");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("unix-paint {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let icon = app_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Paint")
            .with_app_id("unix-paint")
            .with_inner_size([1180.0, 740.0])
            .with_min_inner_size([640.0, 420.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Paint", options, Box::new(move |cc| Ok(Box::new(app::PaintApp::new(cc, file, screenshot, script)))))
}

/// A simple original app icon: four overlapping paint drops on a rounded tile.
fn app_icon() -> egui::IconData {
    app_icon_sized(128)
}

fn app_icon_sized(n: u32) -> egui::IconData {
    let mut px = tiny_skia::Pixmap::new(n, n).unwrap();
    let k = n as f32 / 64.0;
    let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
    let mut pb = tiny_skia::PathBuilder::new();
    shapes::rounded_rect(&mut pb, 2.0 * k, 2.0 * k, 60.0 * k, 60.0 * k, 14.0 * k);
    paint.set_color_rgba8(250, 250, 250, 255);
    px.fill_path(&pb.finish().unwrap(), &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    for (i, (r, g, b)) in [(237, 28, 36), (255, 201, 14), (34, 177, 76), (0, 162, 232)].iter().enumerate() {
        let (cx, cy) = ((22.0 + (i % 2) as f32 * 20.0) * k, (22.0 + (i / 2) as f32 * 20.0) * k);
        paint.set_color_rgba8(*r, *g, *b, 235);
        let c = tiny_skia::PathBuilder::from_circle(cx, cy, 13.0 * k).unwrap();
        px.fill_path(&c, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
    }
    egui::IconData { rgba: px.take_demultiplied(), width: n, height: n }
}
