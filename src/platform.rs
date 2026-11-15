//! OS integration for Linux and macOS: printing through CUPS (`lp`) and setting the desktop
//! wallpaper (GNOME, KDE Plasma, XFCE, macOS).

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Command;
use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WallpaperMode {
    Fill,
    Tile,
    Center,
}

fn data_dir() -> PathBuf {
    let d = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("unix-paint");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn write_png(p: &Pixmap, name: &str) -> Result<PathBuf> {
    let path = data_dir().join(name);
    p.save_png(&path).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let out = Command::new(cmd).args(args).output().with_context(|| format!("running {cmd}"))?;
    if !out.status.success() {
        bail!("{cmd} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn have(cmd: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&paths).any(|dir| dir.join(cmd).is_file())
}

pub fn set_wallpaper(p: &Pixmap, mode: WallpaperMode) -> Result<()> {
    // A fresh file name each time so desktops notice the change.
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for old in std::fs::read_dir(data_dir())?.flatten() {
        if old.file_name().to_string_lossy().starts_with("wallpaper-") {
            let _ = std::fs::remove_file(old.path());
        }
    }
    let path = write_png(p, &format!("wallpaper-{stamp}.png"))?;
    let path_s = path.to_string_lossy().to_string();

    if cfg!(target_os = "macos") {
        let quoted = path_s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("tell application \"System Events\" to tell every desktop to set picture to \"{quoted}\"");
        return run("osascript", &["-e", &script]);
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default().to_lowercase();
    if desktop.contains("kde") && have("plasma-apply-wallpaperimage") {
        // Plasma's fill modes: 2 = scaled & cropped, 3 = tiled, 6 = centered.
        let fill = match mode {
            WallpaperMode::Fill => "2",
            WallpaperMode::Tile => "3",
            WallpaperMode::Center => "6",
        };
        let quoted = path_s.replace('\\', "\\\\").replace('\'', "\\'");
        let script = format!(
            "desktops().forEach(d => {{ d.wallpaperPlugin = 'org.kde.image'; d.currentConfigGroup = ['Wallpaper','org.kde.image','General']; d.writeConfig('Image', 'file://{quoted}'); d.writeConfig('FillMode', {fill}); }})"
        );
        if have("qdbus6") || have("qdbus") {
            let q = if have("qdbus6") { "qdbus6" } else { "qdbus" };
            if run(q, &["org.kde.plasmashell", "/PlasmaShell", "org.kde.PlasmaShell.evaluateScript", &script]).is_ok() {
                return Ok(());
            }
        }
        return run("plasma-apply-wallpaperimage", &[&path_s]);
    }
    if desktop.contains("xfce") && have("xfconf-query") {
        let style = match mode {
            WallpaperMode::Fill => "5",
            WallpaperMode::Tile => "2",
            WallpaperMode::Center => "1",
        };
        let out = Command::new("xfconf-query").args(["-c", "xfce4-desktop", "-l"]).output()?;
        for prop in String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.ends_with("last-image")) {
            run("xfconf-query", &["-c", "xfce4-desktop", "-p", prop, "-s", &path_s])?;
            let sp = prop.replace("last-image", "image-style");
            let _ = run("xfconf-query", &["-c", "xfce4-desktop", "-p", &sp, "-s", style]);
        }
        return Ok(());
    }
    if have("gsettings") {
        let uri = format!("file://{path_s}");
        let opt = match mode {
            WallpaperMode::Fill => "zoom",
            WallpaperMode::Tile => "wallpaper",
            WallpaperMode::Center => "centered",
        };
        run("gsettings", &["set", "org.gnome.desktop.background", "picture-uri", &uri])?;
        let _ = run("gsettings", &["set", "org.gnome.desktop.background", "picture-uri-dark", &uri]);
        let _ = run("gsettings", &["set", "org.gnome.desktop.background", "picture-options", opt]);
        return Ok(());
    }
    if have("feh") {
        let flag = match mode {
            WallpaperMode::Fill => "--bg-fill",
            WallpaperMode::Tile => "--bg-tile",
            WallpaperMode::Center => "--bg-center",
        };
        return run("feh", &[flag, &path_s]);
    }
    bail!("No supported desktop environment was found (GNOME, KDE Plasma, XFCE, feh or macOS).")
}

pub fn list_printers() -> Vec<String> {
    let Ok(out) = Command::new("lpstat").arg("-e").output() else { return vec![] };
    let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    // Put the default destination first.
    if let Ok(d) = Command::new("lpstat").arg("-d").output() {
        let s = String::from_utf8_lossy(&d.stdout);
        if let Some(def) = s.split(':').nth(1).map(|x| x.trim().to_string()) {
            if let Some(i) = v.iter().position(|p| *p == def) {
                let p = v.remove(i);
                v.insert(0, p);
            }
        }
    }
    v
}

pub struct PrintOptions {
    pub printer: Option<String>,
    pub copies: u32,
    pub landscape: bool,
    pub fit: bool,
    pub scale: f32,
    pub center: bool,
    pub margin_mm: f32,
}

pub fn print(p: &Pixmap, title: &str, dpi: f32, o: &PrintOptions) -> Result<()> {
    if !have("lp") {
        bail!("The `lp` command (CUPS) is not available.");
    }
    let path = write_png(p, "print-job.png")?;
    let copies = o.copies.to_string();
    let margin_pt = ((o.margin_mm / 25.4) * 72.0).round().to_string();
    let mut args: Vec<String> = vec!["-t".into(), title.into(), "-n".into(), copies];
    if let Some(d) = &o.printer {
        args.push("-d".into());
        args.push(d.clone());
    }
    for opt in [
        if o.landscape { "landscape".to_string() } else { "portrait".to_string() },
        if o.fit { "fit-to-page".to_string() } else { format!("ppi={}", (dpi * 100.0 / o.scale.max(1.0)).round()) },
        format!("page-left={margin_pt}"),
        format!("page-right={margin_pt}"),
        format!("page-top={margin_pt}"),
        format!("page-bottom={margin_pt}"),
        format!("position={}", if o.center { "center" } else { "top-left" }),
    ] {
        args.push("-o".into());
        args.push(opt);
    }
    args.push(path.to_string_lossy().into_owned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run("lp", &refs)
}
