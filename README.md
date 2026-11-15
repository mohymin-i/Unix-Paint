# Paint for Unix

A from-scratch rebuild of **Windows 11 Paint** for Linux and macOS, written in Rust
([egui](https://github.com/emilk/egui) for the UI, [tiny-skia](https://github.com/RazrFalcon/tiny-skia)
for rasterising, [cosmic-text](https://github.com/pop-os/cosmic-text) for text).

The feature set and layout follow Paint (version 11.2609.401.0, reverse-engineered with Ghidra;
see [`re/FINDINGS.md`](re/FINDINGS.md)). Where an exact copy isn't possible, the look is a cleaner,
modern take. All artwork is original or open-source: Microsoft's icons, brush textures and other
bitmaps are not used.

## Features

| Area | What's there |
|---|---|
| **Selection** | Rectangle and free-form, select all, invert selection, delete, transparent selection (colour 2 becomes see-through), move, resize with 8 handles (Shift keeps the aspect ratio), Ctrl+drag stamps copies, arrow-key nudging |
| **Image** | Crop to selection, resize & skew (percent or pixels, keep aspect ratio), rotate 90°/180°/custom angle, flip horizontally/vertically, invert colours. All of these act on the selection when there is one |
| **Tools** | Pencil (hard-edged), fill bucket, text, eraser, colour picker (switches back to the previous tool, like Paint), magnifier (left click zooms in, right click zooms out) |
| **Brushes** | Brush, calligraphy brush (45°), calligraphy pen (135°), airbrush (keeps spraying while held), oil, crayon, marker, natural pencil, watercolour. Each has its own size and opacity |
| **Shapes** | All 23 Paint shapes: line, curve (drag, then two bends), oval, rectangle, rounded rectangle, polygon, triangle, right triangle, diamond, pentagon, hexagon, four arrows, 4/5/6-point stars, three callouts, heart, lightning. Shapes stay editable (move, resize, change colours/size/outline/fill) until you click elsewhere. Outline and fill can be none, solid, crayon, marker, oil, natural pencil or watercolour. Shift draws squares/circles and 45° lines |
| **Colours** | Colour 1 / colour 2 with an active slot, the 20-colour Paint palette, 10 custom colours, an Edit colours dialog (HSV square, hue bar, RGB, hex). The right mouse button draws with colour 2 |
| **Text** | Any installed font family, point size, bold/italic/underline/strikethrough, left/centre/right alignment, optional filled background. The text box wraps, grows to fit, and can be moved and resized |
| **Layers** | Add, duplicate, delete, merge down, reorder, rename, show/hide, background colour and visibility, flatten |
| **View** | Zoom 1–3200% (Ctrl+wheel zooms at the pointer), fit to window, rulers, gridlines (pixel grid at ≥400%), status bar, thumbnail window, full screen, light/dark/system theme |
| **Canvas** | Drag the canvas edges to resize. Pan with Space+drag or the middle mouse button |
| **Files** | Open PNG, JPEG, BMP, GIF, TIFF, WEBP, ICO, TGA, PNM and QOI. Save as PNG, JPEG, BMP, GIF, TIFF, WEBP, ICO or a layered `.paint` project. Import a picture onto the canvas, recent files, drag and drop (Ctrl+drop pastes), unsaved-changes prompt |
| **Edit** | Undo/redo (50 levels, like Paint), cut/copy/paste through the system clipboard (pasting a large picture grows the canvas), clear image |
| **System** | Print through CUPS (`lp`): printer, copies, orientation, fit-to-page or scale, margins, centring, live preview. Set as desktop background (fill/tile/centre) on GNOME, KDE Plasma, XFCE, feh and macOS. Image properties: size in px/in/cm, DPI, black-and-white mode, transparency |

Keyboard shortcuts match Paint (Ctrl+N/O/S/P/E/W/Z/Y/A, Ctrl+Shift+X crop, Ctrl+Shift+I invert,
Ctrl+R rulers, Ctrl+G grid, F11 full screen, F12 save as, and so on). Press **F1** in the app for
the full list.

### Not included

- **AI features** (Cocreator, Image Creator, generative fill/erase, background removal, stickers,
  coloring book). These depend on Microsoft cloud services and NPU models.
- **Windows-only integration**: OLE embedding, scanner/camera import, the Store codec installer.
- **`.paint` compatibility**: Microsoft's project format lives in `ProjectFile.dll`, which wasn't
  available to analyse. This rebuild's `.paint` files are layered too, but they use their own
  format (documented in `src/io.rs`).

## Building

You need Rust (stable, edition 2024) and a C toolchain for linking.

```sh
cargo build --release
./target/release/unix-paint [FILE]
```

- **Linux:** a normal `cc` is enough. No GTK or other `-dev` packages are needed: file dialogs use
  the XDG desktop portal and the clipboard works on both Wayland and X11. To install for the
  current user (binary, `.desktop` entry, icon): `packaging/install-linux.sh`.
- **macOS:** install the Xcode command line tools, then run `packaging/macos-bundle.sh` to build
  `target/Paint.app`.

If there's no system C compiler (as on the machine this was developed on), you can use
[Zig](https://ziglang.org) as the linker: point `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER` at a
small wrapper that runs `zig cc -target x86_64-linux-gnu.2.35 "$@"`.

## Testing

```sh
cargo test                                   # unit tests: raster ops, brushes, shapes, history, I/O, text
./target/release/unix-paint --script tests/scripts/tour.txt   # drive the real UI
```

`--script` replays pointer, keyboard and command input through the real UI and can take
screenshots and save files along the way. The command language is documented in
`src/script.rs`. Scripted runs never read or write your settings.

## Project layout

```
src/
  app.rs          application state, commands, texture compositing
  doc.rs          document: layers over a background, compositing
  history.rs      undo/redo (rect patches + whole-document snapshots)
  brush.rs        stroke engine: pencil, 9 brushes, eraser, textured shape media
  shapes.rs       the 23 auto-shapes
  shape_edit.rs   editable pending shapes
  selection.rs    rectangle/free-form/inverted selections, floating content
  text.rs         text box editing and rendering
  raster.rs       flood fill, flip/rotate/resize/skew, invert, conversions
  io.rs           image formats, .paint projects, clipboard
  platform.rs     printing (CUPS), desktop wallpaper
  settings.rs     persisted preferences (~/.config/unix-paint/settings.json)
  script.rs       scripted UI driver for tests
  ui/             toolbar, canvas, menus/status bar/layers panel, dialogs, theme, shortcuts
re/               reverse-engineering notes and scripts (Ghidra, PE resources)
packaging/        .desktop file, Linux install script, macOS bundle script
```

## Credits

Icons: [Phosphor Icons](https://phosphoricons.com) (MIT, see `assets/fonts/PHOSPHOR-LICENSE-MIT.txt`).
Not affiliated with or endorsed by Microsoft. "Paint" and "Windows" are trademarks of Microsoft.
