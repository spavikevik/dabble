//! App actions, plus the native macOS menu bar that carries their shortcuts.
//!
//! The menu bar matters for more than looks: egui-winit swallows Cmd+V when the
//! clipboard holds an image instead of text, so image paste needs a real
//! menu key equivalent.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    ZoomIn,
    ZoomOut,
    ZoomActual,
    ZoomFit,
    Crop,
    ResizeSkew,
    RotateRight,
    RotateLeft,
    Rotate180,
    FlipHorizontal,
    FlipVertical,
    ImageProperties,
}

impl Action {
    const ALL: [Action; 24] = [
        Action::New,
        Action::Open,
        Action::Save,
        Action::SaveAs,
        Action::Quit,
        Action::Undo,
        Action::Redo,
        Action::Cut,
        Action::Copy,
        Action::Paste,
        Action::Delete,
        Action::SelectAll,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomActual,
        Action::ZoomFit,
        Action::Crop,
        Action::ResizeSkew,
        Action::RotateRight,
        Action::RotateLeft,
        Action::Rotate180,
        Action::FlipHorizontal,
        Action::FlipVertical,
        Action::ImageProperties,
    ];

    fn id(self) -> String {
        format!("daub.{self:?}")
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::New => "New",
            Action::Open => "Open…",
            Action::Save => "Save",
            Action::SaveAs => "Save As…",
            Action::Quit => "Quit Daub",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::Cut => "Cut",
            Action::Copy => "Copy",
            Action::Paste => "Paste",
            Action::Delete => "Delete",
            Action::SelectAll => "Select All",
            Action::ZoomIn => "Zoom In",
            Action::ZoomOut => "Zoom Out",
            Action::ZoomActual => "Actual Size",
            Action::ZoomFit => "Fit to Window",
            Action::Crop => "Crop to Selection",
            Action::ResizeSkew => "Resize and Skew…",
            Action::RotateRight => "Rotate Right 90°",
            Action::RotateLeft => "Rotate Left 90°",
            Action::Rotate180 => "Rotate 180°",
            Action::FlipHorizontal => "Flip Horizontal",
            Action::FlipVertical => "Flip Vertical",
            Action::ImageProperties => "Image Properties…",
        }
    }

    pub fn shortcut(self) -> Option<&'static str> {
        Some(match self {
            Action::New => "CmdOrCtrl+N",
            Action::Open => "CmdOrCtrl+O",
            Action::Save => "CmdOrCtrl+S",
            Action::SaveAs => "CmdOrCtrl+Shift+S",
            Action::Quit => "CmdOrCtrl+Q",
            Action::Undo => "CmdOrCtrl+Z",
            Action::Redo => "CmdOrCtrl+Shift+Z",
            Action::Cut => "CmdOrCtrl+X",
            Action::Copy => "CmdOrCtrl+C",
            Action::Paste => "CmdOrCtrl+V",
            Action::SelectAll => "CmdOrCtrl+A",
            Action::ZoomIn => "CmdOrCtrl+Equal",
            Action::ZoomOut => "CmdOrCtrl+Minus",
            Action::ZoomActual => "CmdOrCtrl+0",
            Action::ZoomFit => "CmdOrCtrl+9",
            Action::Crop => "CmdOrCtrl+K",
            Action::ResizeSkew => "CmdOrCtrl+Alt+I",
            Action::RotateRight => "CmdOrCtrl+R",
            Action::RotateLeft => "CmdOrCtrl+L",
            Action::ImageProperties => "CmdOrCtrl+E",
            Action::Delete | Action::Rotate180 | Action::FlipHorizontal | Action::FlipVertical => return None,
        })
    }

    /// Human-readable shortcut for in-app menus/tooltips.
    pub fn shortcut_text(self) -> String {
        self.shortcut()
            .map(|s| {
                s.replace("CmdOrCtrl+", "⌘")
                    .replace("Shift+", "⇧")
                    .replace("Alt+", "⌥")
                    .replace("Equal", "=")
                    .replace("Minus", "-")
            })
            .unwrap_or_default()
    }
}

#[cfg(target_os = "macos")]
pub struct NativeMenu {
    _menu: muda::Menu,
    rx: std::sync::mpsc::Receiver<Action>,
}

#[cfg(target_os = "macos")]
impl NativeMenu {
    pub fn install(ctx: &egui::Context) -> Option<Self> {
        use muda::{Menu, MenuItem, PredefinedMenuItem as P, Submenu, accelerator::Accelerator};

        let item = |a: Action| {
            let acc = a.shortcut().and_then(|s| s.parse::<Accelerator>().ok());
            MenuItem::with_id(a.id(), a.label(), true, acc)
        };
        let menu = Menu::new();
        let app = Submenu::new("Daub", true);
        app.append_items(&[
            &P::about(Some("About Daub"), None),
            &P::separator(),
            &P::services(None),
            &P::separator(),
            &P::hide(None),
            &P::hide_others(None),
            &P::show_all(None),
            &P::separator(),
            &item(Action::Quit),
        ])
        .ok()?;
        let file = Submenu::new("File", true);
        file.append_items(&[
            &item(Action::New),
            &item(Action::Open),
            &P::separator(),
            &item(Action::Save),
            &item(Action::SaveAs),
        ])
        .ok()?;
        let edit = Submenu::new("Edit", true);
        edit.append_items(&[
            &item(Action::Undo),
            &item(Action::Redo),
            &P::separator(),
            &item(Action::Cut),
            &item(Action::Copy),
            &item(Action::Paste),
            &item(Action::Delete),
            &P::separator(),
            &item(Action::SelectAll),
        ])
        .ok()?;
        let view = Submenu::new("View", true);
        view.append_items(&[
            &item(Action::ZoomIn),
            &item(Action::ZoomOut),
            &item(Action::ZoomActual),
            &item(Action::ZoomFit),
            &P::separator(),
            &P::fullscreen(None),
        ])
        .ok()?;
        let image = Submenu::new("Image", true);
        image.append_items(&[
            &item(Action::Crop),
            &item(Action::ResizeSkew),
            &P::separator(),
            &item(Action::RotateRight),
            &item(Action::RotateLeft),
            &item(Action::Rotate180),
            &item(Action::FlipHorizontal),
            &item(Action::FlipVertical),
            &P::separator(),
            &item(Action::ImageProperties),
        ])
        .ok()?;
        let window = Submenu::new("Window", true);
        window.append_items(&[&P::minimize(None), &P::maximize(None)]).ok()?;
        menu.append_items(&[&app, &file, &edit, &image, &view, &window]).ok()?;
        menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();

        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        muda::MenuEvent::set_event_handler(Some(move |e: muda::MenuEvent| {
            if let Some(a) = Action::ALL.into_iter().find(|a| a.id() == e.id.0) {
                let _ = tx.send(a);
                ctx.request_repaint();
            }
        }));
        Some(Self { _menu: menu, rx })
    }

    pub fn drain(&self) -> Vec<Action> {
        self.rx.try_iter().collect()
    }
}

#[cfg(not(target_os = "macos"))]
pub struct NativeMenu;

#[cfg(not(target_os = "macos"))]
impl NativeMenu {
    pub fn install(_ctx: &egui::Context) -> Option<Self> {
        None
    }
    pub fn drain(&self) -> Vec<Action> {
        vec![]
    }
}

/// Fallback keyboard shortcuts when no native menu is installed.
pub fn egui_shortcuts(ctx: &egui::Context) -> Vec<Action> {
    use egui::{Key, KeyboardShortcut as K, Modifiers as M};
    let table = [
        (K::new(M::COMMAND | M::SHIFT, Key::S), Action::SaveAs),
        (K::new(M::COMMAND | M::SHIFT, Key::Z), Action::Redo),
        (K::new(M::COMMAND, Key::N), Action::New),
        (K::new(M::COMMAND, Key::O), Action::Open),
        (K::new(M::COMMAND, Key::S), Action::Save),
        (K::new(M::COMMAND, Key::Q), Action::Quit),
        (K::new(M::COMMAND, Key::Z), Action::Undo),
        (K::new(M::COMMAND, Key::Y), Action::Redo),
        (K::new(M::COMMAND, Key::A), Action::SelectAll),
        (K::new(M::COMMAND, Key::Equals), Action::ZoomIn),
        (K::new(M::COMMAND, Key::Minus), Action::ZoomOut),
        (K::new(M::COMMAND, Key::Num0), Action::ZoomActual),
        (K::new(M::COMMAND, Key::Num9), Action::ZoomFit),
        (K::new(M::COMMAND | M::ALT, Key::I), Action::ResizeSkew),
        (K::new(M::COMMAND, Key::K), Action::Crop),
        (K::new(M::COMMAND, Key::R), Action::RotateRight),
        (K::new(M::COMMAND, Key::L), Action::RotateLeft),
        (K::new(M::COMMAND, Key::E), Action::ImageProperties),
    ];
    let mut out = vec![];
    ctx.input_mut(|i| {
        for (k, a) in table {
            if i.consume_shortcut(&k) {
                out.push(a);
            }
        }
        for e in &i.events {
            match e {
                egui::Event::Copy => out.push(Action::Copy),
                egui::Event::Cut => out.push(Action::Cut),
                egui::Event::Paste(_) => out.push(Action::Paste),
                _ => {}
            }
        }
    });
    out
}
