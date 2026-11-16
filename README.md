# Dabble

A simple paint app for macOS in Rust. It follows the layout and behavior of Windows 11 Paint, but it is not a Microsoft product and uses none of Microsoft's assets.

## Run

```sh
cargo run --release            # empty canvas
cargo run --release -- pic.png # open a file
scripts/bundle.sh              # build target/release/Dabble.app (with icon), then drag it to Applications
```

## What works

- Canvas: zoom (pinch, ⌘-scroll, ⌘=/⌘-, ⌘0 for 100%, ⌘9 to fit, magnifier tool, zoom slider), pan (scroll, middle-drag, Space+drag)
- Brushes: brush, two calligraphy brushes, airbrush (keeps spraying while held), oil, crayon, marker, natural pencil, watercolor. A stroke never darkens where it crosses itself, and only the changed area is redrawn, so long strokes stay fast.
- Tools: pencil, brushes, eraser (right-drag replaces only Color 1 with Color 2), fill, color picker, magnifier, text
- Two colors: left button uses Color 1, right button uses Color 2. Palette, 10 custom slots, color editor
- Shapes: 20 shapes, including a curve (drag, then drag twice to bend) and a polygon (drag the first side, click to add corners, click the start to close). Outline and fill can each be none, solid, crayon, marker, oil, natural pencil or watercolor. Each shape stays editable after you draw it (resize handles, move, live color/size changes) until you click away, press Enter/Esc, or switch tools. Hold Shift to draw squares or 45° lines.
- Selection: rectangle and free-form. Move, delete, select all, transparent selection, cut/copy/paste through the system clipboard (pasting a larger image grows the canvas)
- Text: system fonts, size, bold/italic/underline/strikethrough, transparent or filled background, word wrap, arrow keys, paste
- Undo/redo (stores only the changed area), unsaved-changes prompt, open/save PNG, JPEG, BMP, GIF, TIFF, WebP, drag-and-drop to open
- Image: crop to selection (⌘K), resize and skew by percent or pixels (⌥⌘I), rotate right/left (⌘R/⌘L) and 180°, flip horizontal/vertical, image properties (⌘E). These act on the selection if there is one, otherwise on the whole image.
- Drag the selection's handles to scale it. Drag the canvas's right, bottom or corner handle to resize the canvas.
- Transparent background (Image menu or Image Properties): eraser, Delete and new canvas space leave transparent pixels. Turns on by itself when you open an image that has transparency.
- Layers (⇧⌘L): add, duplicate, delete, reorder, merge down, rename (double-click), hide, opacity. Tools work on the selected layer; rotate/flip/resize/crop of the whole image apply to every layer. Saving flattens the visible layers. The eraser leaves transparent pixels on layers above the background.
- Light and dark appearance, following macOS by default (View menu to override)
- Two interfaces (Settings, ⌘,): the Paint-style ribbon, or a native macOS one with an `NSToolbar` and a Liquid Glass inspector built from real AppKit controls (tool options, color wells, palette, layers). On macOS before 26 the inspector uses the sidebar material instead of Liquid Glass. Settings are saved to `~/Library/Application Support/Dabble/settings.txt`. To try an interface without saving it: `DABBLE_INTERFACE=native` (or `classic`), `DABBLE_THEME=light`/`dark`.
- Native macOS menu bar with standard shortcuts

## Tests

```sh
cargo test                                                        # unit tests + headless UI interaction tests
DABBLE_SHOT_DIR=/tmp cargo test render_screenshots -- --ignored     # render the UI offscreen to PNGs
```

## Dependencies and licenses

All MIT and/or Apache-2.0: egui/eframe, objc2 / objc2-app-kit / objc2-foundation and raw-window-handle (macOS interface), egui_kittest (dev), tiny-skia (BSD-3-Clause), image, rfd, arboard, muda, ab_glyph, fontdb.
Icons come from Phosphor Icons (MIT) via `egui-phosphor`.
The app icon is original, drawn in code by `examples/make_icon.rs`.
