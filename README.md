# Daub

A simple paint app for macOS in Rust. It follows the layout and behavior of Windows 11 Paint, but it is not a Microsoft product and uses none of Microsoft's assets.

## Run

```sh
cargo run --release            # empty canvas
cargo run --release -- pic.png # open a file
```

## What works

- Canvas: zoom (pinch, ⌘-scroll, ⌘=/⌘-, ⌘0 for 100%, ⌘9 to fit, magnifier tool, zoom slider), pan (scroll, middle-drag, Space+drag)
- Tools: pencil, brush, eraser (right-drag replaces only Color 1 with Color 2), fill, color picker, magnifier, text
- Two colors: left button uses Color 1, right button uses Color 2. Palette, 10 custom slots, color editor
- Shapes: 18 shapes with outline and fill options. Each shape stays editable after you draw it (resize handles, move, live color/size changes) until you click away, press Enter/Esc, or switch tools. Hold Shift to draw squares or 45° lines.
- Selection: rectangle and free-form. Move, delete, select all, transparent selection, cut/copy/paste through the system clipboard (pasting a larger image grows the canvas)
- Text: system fonts, size, bold/italic/underline/strikethrough, transparent or filled background, word wrap, arrow keys, paste
- Undo/redo (stores only the changed area), unsaved-changes prompt, open/save PNG, JPEG, BMP, GIF, TIFF, WebP, drag-and-drop to open
- Image: crop to selection (⌘K), resize and skew by percent or pixels (⌥⌘I), rotate right/left (⌘R/⌘L) and 180°, flip horizontal/vertical, image properties (⌘E). These act on the selection if there is one, otherwise on the whole image.
- Drag the selection's handles to scale it. Drag the canvas's right, bottom or corner handle to resize the canvas.
- Transparent background (Image menu or Image Properties): eraser, Delete and new canvas space leave transparent pixels. Turns on by itself when you open an image that has transparency.
- Native macOS menu bar with standard shortcuts

## Tests

```sh
cargo test                                                        # unit tests + headless UI interaction tests
DAUB_SHOT_DIR=/tmp cargo test render_screenshots -- --ignored     # render the UI offscreen to PNGs
```

## Dependencies and licenses

All MIT and/or Apache-2.0: egui/eframe, egui_kittest (dev), tiny-skia (BSD-3-Clause), image, rfd, arboard, muda, ab_glyph, fontdb.
Icons come from Phosphor Icons (MIT) via `egui-phosphor`.
