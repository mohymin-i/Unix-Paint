# mspaint.exe — reverse-engineering findings

Binary: `mspaint.exe` 11.2609.401.0 (Windows 11 Paint, PE32+ x86‑64, MFC140 + WinUI 3 XAML islands,
Direct2D/GDI+). No PDB; analysis via Ghidra 12.2 headless (`re/tools/ExportDecomp.java`) and a
small PE parser (`re/tools/pe_dump.py`). Raw output lives in `re/out/` and `re/decomp/`
(git-ignored: regenerate with the scripts).

## Architecture

The exe is the MFC shell (`CPBApp`, `CPBFrame`, `CPBDoc`, `CPBView`, `CPBSrvrItem` — "PBrush" OLE
server legacy) that hosts a XAML UI (`PaintUI.*` view-models: `AppChrome`, `LayersPanel`,
`EditColorsDialog`, `ResizeAndSkewDialog`, `ImagePropertiesDialog`, `RotateCustomDialog`,
`CanvasThumbnail`, …). Most logic is in companion DLLs that are **not** in this folder; their
interfaces are recoverable from the import table:

| DLL | Role (from imported symbols) |
|---|---|
| BasePaint | `Image` (GDI+/D2D bitmap): Resize(mode, direction), Crop(rect/region), Rotate(angle), H/V skew, H/V flip, InvertColors, HasTransparentPixels; `ShapeFactory`; `CAttributeStore` settings; telemetry |
| PaintTools | Tool classes (see below), selection masks, invert selection, select all |
| PGS | "skeletal stroke" brush engine (texture strip bent along a path) |
| ProjectFile | `.paint` layered project format: `LayerStack` (dpi x/y, background colour + visibility, preview bitmap) of `ImageLayer` (bitmap, name, visible) |
| ImageProcessing | GDI+ codecs load/save |
| RestartRecovery | crash/restart autosave |
| AI* / PaintAIManager | Cocreator, Image Creator, generative fill/erase, background removal, stickers, coloring book (cloud/NPU) — out of scope |

## Tools (id → class, from the tool factory `FUN_140051a00`)

| id | tool | notes |
|---|---|---|
| 0x259 | Rectangle selection | |
| 0x25a | Free-form selection | |
| 0x2f0 | Object select (AI) | out of scope |
| 0x25b | Text | font family, point size, bold/italic/underline/strikeout, alignment, fill-background |
| 0x25c | Fill (flood) | |
| 0x25d | Magnifier | |
| 0x25e | Color picker | |
| 0x25f | Eraser | |
| 0x262 | Pencil | |
| 0x28b | Brush (round) | |
| 0x28c / 0x28d | Calligraphy 45° / 135° | cross-section 2/3 |
| 0x28e | Airbrush | |
| 0x28f | Oil brush | ToolMedia 2 |
| 0x290 | Crayon | ToolMedia 6 |
| 0x291 | Marker | ToolMedia 8 |
| 0x292 | Natural pencil | ToolMedia 7 |
| 0x293 | Watercolor | ToolMedia 4 |
| 0x294 | Generative erase (AI) | out of scope |
| 0x2bd | Line | |
| 0x2be | Curve (Bezier, 2 control points) | |
| 0x2c2 | Polygon | |
| 0x2bf.. | Auto-shapes: rectangle, rounded rect, ellipse, triangle, right triangle, diamond, pentagon, hexagon, oval/cloud/rounded-rect callouts, left/right/up/down arrows, 4/5/6-point stars, heart, lightning | `ShapeFactory::Create*` |

Shape outline/fill use a **ToolMedia** enum (1..8): 0 none, 1 solid, 2 oil, 4 watercolor,
6 crayon, 7 natural pencil, 8 marker. Defaults: outline = solid (1), fill = none (0).

## Settings store (`CAttributeStore`, `FUN_140076950`) — id: default [min, max]

- 1: 5 [1, 50]; 2: 3, 3: 1, 4: 5, 5: 5, 8: 4, 12: 8, 13: 64 — all [1, 248] (per-tool stroke sizes)
- 6, 7, 9, 10: 30 [4, 496]; 11: 8 [4, 496] (large tools: textured brushes, eraser, airbrush)
- 31–34: colours (u32, theme-dependent default — black/white)
- 41–43: int flags [0, 1]
- 51/52: shape outline/fill ToolMedia, default 1 / 0, range [1, 8]
- 61: selection type 1 (rect) [1, 2]; 62–64: bool (transparent selection etc.)
- 71, 120–131, 140: doubles [0, 1] — per-tool **opacity** (120+ = per brush)
- 101–106: text/print/view doubles

## Brush engine

Textured brushes ("skeletal strokes") pick one of three texture strips by size band
(≤30 px, ≤90 px, ≤496 px). Each brush has a 512×512 grayscale paper/grain texture plus stroke
strips (e.g. 1427×496 RGBA). The rebuild synthesises its own textures procedurally instead of
shipping Microsoft's bitmaps.

## File handling

- Formats via GDI+: BMP (mono/16/256/24-bit), JPEG, GIF, TIFF, PNG, ICO, HEIC/WEBP (codec
  extensions), `.paint` project (layers). Filters: `*.bmp;*.dib;*.rle`, "All picture files".
- Save warns on colour loss (`ColorSaveWarning`) and on flattening layers (`TitleFlattenLayers`).
- Recovery/restart, "Set as desktop background" (fill/tile/center), print + page setup
  (orientation, margins, centering, scale adjust-to % / fit-to pages), image properties
  (units, colour/BW, DPI), `/p` & `/pt` print command-line switches.

## UI strings of note

Undo-stack action names: Resize, Horizontal/Vertical flip, Invert colors, Horizontal/Vertical skew,
Rotate, Clear, Insert/Remove/Duplicate/Collapse/Move layer, Draw text, …
Announcements: Paste, Copy, Delete, Cut, Flip, Rotate, Invert selection, Select all, Undo, Redo,
Cropped, Brush size increased/decreased.
