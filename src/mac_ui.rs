//! Optional native macOS interface: an `NSToolbar` and a Liquid Glass
//! inspector built from real AppKit controls. egui still draws the canvas
//! and status bar underneath.
//!
//! Native controls talk to the app through `NativeEvent`s sent from an
//! Objective-C target object; each frame `DabbleApp::sync_native` pushes app
//! state back into the controls when it changed.

use egui::Color32;

use crate::app::{SelectMode, Tool};
use crate::brushes::BrushKind;
use crate::menu::Action;
use crate::shapes::{PaintStyle, ShapeKind};

pub enum NativeEvent {
    Action(Action),
    Tool(Tool),
    SelectMode(SelectMode),
    TransparentSelection(bool),
    Brush(BrushKind),
    Shape(ShapeKind),
    Outline(PaintStyle),
    Fill(PaintStyle),
    Size(f32),
    Color1(Color32),
    Color2(Color32),
    /// A palette swatch; `true` when Option-clicked (sets Color 2).
    Palette(Color32, bool),
    Layer(LayerCmd),
    Font(usize),
    FontSize(f32),
    /// Bold, italic, underline, strikethrough.
    TextStyle([bool; 4]),
    TextBackground(bool),
    ToggleInspector,
}

pub enum LayerCmd {
    Select(usize),
    ToggleVisible(usize),
    Opacity(f32),
    Add,
    Duplicate,
    Delete,
    Up,
    Down,
    Merge,
    Rename,
}

pub const TOOLS: [Tool; 9] = [
    Tool::Select,
    Tool::Pencil,
    Tool::Fill,
    Tool::Text,
    Tool::Eraser,
    Tool::Picker,
    Tool::Magnifier,
    Tool::Brush,
    Tool::Shape,
];

#[cfg(target_os = "macos")]
pub use imp::*;

#[cfg(not(target_os = "macos"))]
pub struct NativeUi;

#[cfg(not(target_os = "macos"))]
impl NativeUi {
    pub fn drain(&self) -> Vec<NativeEvent> {
        vec![]
    }
}

#[cfg(not(target_os = "macos"))]
pub fn prompt_text(_title: &str, _value: &str) -> Option<String> {
    None
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;
    use std::sync::mpsc::{Receiver, Sender, channel};

    use egui::Color32;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, ProtocolObject};
    use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
    use objc2_app_kit::*;
    use objc2_foundation::{
        MainThreadMarker, NSCopying, NSArray, NSData, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, ns_string,
    };

    use super::{LayerCmd, NativeEvent, TOOLS};
    use crate::app::{DabbleApp, SelectMode, Tool};
    use crate::brushes::BrushKind;
    use crate::menu::Action;
    use crate::shapes::{PaintStyle, ShapeKind};
    use crate::{raster, ribbon};

    // Tag categories: tag = category * 1000 + index.
    const T_ACTION: isize = 1;
    const T_TOOL: isize = 2;
    const T_SELECT_MODE: isize = 3;
    const T_TRANSPARENT_SEL: isize = 4;
    const T_BRUSH: isize = 5;
    const T_SHAPE: isize = 6;
    const T_OUTLINE: isize = 7;
    const T_FILL: isize = 8;
    const T_SIZE: isize = 9;
    const T_COLOR1: isize = 10;
    const T_COLOR2: isize = 11;
    const T_PALETTE: isize = 12;
    const T_LAYER_SELECT: isize = 13;
    const T_LAYER_EYE: isize = 14;
    const T_LAYER_OPACITY: isize = 15;
    const T_LAYER_OP: isize = 16;
    const T_FONT: isize = 17;
    const T_FONT_SIZE: isize = 18;
    const T_TEXT_STYLE: isize = 19;
    const T_TEXT_BG: isize = 20;
    const T_INSPECTOR: isize = 21;

    const PANEL_WIDTH: f64 = 268.0;
    const PANEL_MARGIN: f64 = 10.0;
    /// Height of egui's status bar, which the panel stays above.
    const STATUS_BAR: f64 = 26.0;

    pub struct TargetIvars {
        tx: Sender<NativeEvent>,
        ctx: egui::Context,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; Target has no Drop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = TargetIvars]
        pub struct Target;

        unsafe impl NSObjectProtocol for Target {}

        impl Target {
            #[unsafe(method(fire:))]
            fn fire(&self, sender: &AnyObject) {
                if let Some(ev) = event_for(sender, self.mtm()) {
                    let _ = self.ivars().tx.send(ev);
                    self.ivars().ctx.request_repaint();
                }
            }
        }
    );

    impl Target {
        fn new(mtm: MainThreadMarker, tx: Sender<NativeEvent>, ctx: egui::Context) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(TargetIvars { tx, ctx });
            // SAFETY: NSObject's init has this signature.
            unsafe { msg_send![super(this), init] }
        }
    }

    fn color32(c: &NSColor) -> Option<Color32> {
        let c = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
        let f = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        Some(Color32::from_rgb(f(c.redComponent()), f(c.greenComponent()), f(c.blueComponent())))
    }

    fn nscolor(c: Color32) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            c.r() as f64 / 255.0,
            c.g() as f64 / 255.0,
            c.b() as f64 / 255.0,
            1.0,
        )
    }

    /// Turn a control's action into an event, reading the control's value.
    fn event_for(sender: &AnyObject, mtm: MainThreadMarker) -> Option<NativeEvent> {
        let tag = if let Some(m) = sender.downcast_ref::<NSMenuItem>() {
            m.tag()
        } else if let Some(t) = sender.downcast_ref::<NSToolbarItem>() {
            t.tag()
        } else {
            sender.downcast_ref::<NSControl>()?.tag()
        };
        let (cat, idx) = (tag / 1000, (tag % 1000) as usize);
        let popup = || sender.downcast_ref::<NSPopUpButton>().map(|p| p.indexOfSelectedItem().max(0) as usize);
        let value = || sender.downcast_ref::<NSControl>().map(|c| c.doubleValue());
        let on = || sender.downcast_ref::<NSButton>().map(|b| b.state() == NSControlStateValueOn);
        let well = || sender.downcast_ref::<NSColorWell>().and_then(|w| color32(&w.color()));
        Some(match cat {
            T_ACTION => NativeEvent::Action(*Action::ALL.get(idx)?),
            T_TOOL => {
                let seg = sender.downcast_ref::<NSSegmentedControl>()?.selectedSegment();
                NativeEvent::Tool(*TOOLS.get(seg as usize)?)
            }
            T_SELECT_MODE => {
                NativeEvent::SelectMode(if popup()? == 0 { SelectMode::Rect } else { SelectMode::Free })
            }
            T_TRANSPARENT_SEL => NativeEvent::TransparentSelection(on()?),
            T_BRUSH => NativeEvent::Brush(*BrushKind::ALL.get(popup()?)?),
            T_SHAPE => NativeEvent::Shape(*ShapeKind::ALL.get(popup()?)?),
            T_OUTLINE => NativeEvent::Outline(*PaintStyle::ALL.get(popup()?)?),
            T_FILL => NativeEvent::Fill(*PaintStyle::ALL.get(popup()?)?),
            T_SIZE => NativeEvent::Size(value()?.round() as f32),
            T_COLOR1 => NativeEvent::Color1(well()?),
            T_COLOR2 => NativeEvent::Color2(well()?),
            T_PALETTE => {
                let c = ribbon::PALETTE.get(idx)?;
                let option = NSApplication::sharedApplication(mtm)
                    .currentEvent()
                    .is_some_and(|e| e.modifierFlags().contains(NSEventModifierFlags::Option));
                NativeEvent::Palette(Color32::from_rgb(c[0], c[1], c[2]), option)
            }
            T_LAYER_SELECT => NativeEvent::Layer(LayerCmd::Select(idx)),
            T_LAYER_EYE => NativeEvent::Layer(LayerCmd::ToggleVisible(idx)),
            T_LAYER_OPACITY => NativeEvent::Layer(LayerCmd::Opacity(value()? as f32 / 100.0)),
            T_LAYER_OP => NativeEvent::Layer(match idx {
                0 => LayerCmd::Add,
                1 => LayerCmd::Duplicate,
                2 => LayerCmd::Delete,
                3 => LayerCmd::Up,
                4 => LayerCmd::Down,
                5 => LayerCmd::Merge,
                _ => LayerCmd::Rename,
            }),
            T_FONT => NativeEvent::Font(popup()?),
            T_FONT_SIZE => NativeEvent::FontSize(*ribbon::FONT_SIZES.get(popup()?)?),
            T_TEXT_STYLE => {
                let seg = sender.downcast_ref::<NSSegmentedControl>()?;
                NativeEvent::TextStyle(std::array::from_fn(|i| seg.isSelectedForSegment(i as isize)))
            }
            T_TEXT_BG => NativeEvent::TextBackground(popup()? == 1),
            T_INSPECTOR => NativeEvent::ToggleInspector,
            _ => return None,
        })
    }

    // ------------------------------------------------------------ toolbar

    pub struct ToolbarIvars {
        items: RefCell<Vec<(Retained<NSString>, Retained<NSToolbarItem>)>>,
        order: RefCell<Vec<Retained<NSString>>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = ToolbarIvars]
        pub struct ToolbarDelegate;

        unsafe impl NSObjectProtocol for ToolbarDelegate {}

        unsafe impl NSToolbarDelegate for ToolbarDelegate {
            #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
            fn item_for(&self, _toolbar: &NSToolbar, id: &NSString, _insert: bool) -> Option<Retained<NSToolbarItem>> {
                self.ivars().items.borrow().iter().find(|(k, _)| **k == *id).map(|(_, v)| v.clone())
            }

            #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
            fn default_ids(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
                NSArray::from_retained_slice(&self.ivars().order.borrow())
            }

            #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
            fn allowed_ids(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
                NSArray::from_retained_slice(&self.ivars().order.borrow())
            }
        }
    );

    impl ToolbarDelegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm)
                .set_ivars(ToolbarIvars { items: RefCell::new(vec![]), order: RefCell::new(vec![]) });
            // SAFETY: NSObject's init has this signature.
            unsafe { msg_send![super(this), init] }
        }

        fn push(&self, item: Retained<NSToolbarItem>) {
            let id = item.itemIdentifier();
            self.ivars().order.borrow_mut().push(id.clone());
            self.ivars().items.borrow_mut().push((id, item));
        }

        fn push_id(&self, id: &NSString) {
            self.ivars().order.borrow_mut().push(id.copy());
        }
    }

    // ------------------------------------------------------------ helpers

    fn symbol(name: &str) -> Option<Retained<NSImage>> {
        NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), None)
    }

    fn png_image(pm: &tiny_skia::Pixmap, points: (f64, f64)) -> Option<Retained<NSImage>> {
        let bytes = pm.encode_png().ok()?;
        let data = NSData::with_bytes(&bytes);
        let img = NSImage::initWithData(NSImage::alloc(), &data)?;
        img.setSize(NSSize::new(points.0, points.1));
        Some(img)
    }

    fn swatch_image(c: Color32) -> Option<Retained<NSImage>> {
        let mut pm = tiny_skia::Pixmap::new(36, 36)?;
        let circle = tiny_skia::PathBuilder::from_circle(18.0, 18.0, 16.0)?;
        pm.fill_path(&circle, &raster::paint(c, true), tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        let stroke = tiny_skia::Stroke { width: 1.5, ..Default::default() };
        pm.stroke_path(
            &circle,
            &raster::paint(Color32::from_black_alpha(60), true),
            &stroke,
            tiny_skia::Transform::identity(),
            None,
        );
        png_image(&pm, (18.0, 18.0))
    }

    fn label(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
        NSTextField::labelWithString(&NSString::from_str(text), mtm)
    }

    fn heading(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let l = label(text, mtm);
        l.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
        l.setTextColor(Some(&NSColor::secondaryLabelColor()));
        l
    }

    fn hint(text: &str, mtm: MainThreadMarker) -> Retained<NSTextField> {
        let l = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
        l.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        l.setTextColor(Some(&NSColor::secondaryLabelColor()));
        l.setPreferredMaxLayoutWidth(PANEL_WIDTH - 32.0);
        l
    }

    fn stack(vertical: bool, spacing: f64, views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
        let s = NSStackView::new(mtm);
        s.setOrientation(if vertical {
            NSUserInterfaceLayoutOrientation::Vertical
        } else {
            NSUserInterfaceLayoutOrientation::Horizontal
        });
        s.setAlignment(if vertical { NSLayoutAttribute::Leading } else { NSLayoutAttribute::CenterY });
        s.setSpacing(spacing);
        for v in views {
            s.addArrangedSubview(v);
        }
        s
    }

    fn wire(c: &NSControl, target: &Target, tag: isize) {
        c.setTag(tag);
        // SAFETY: `fire:` takes one object argument, as AppKit actions do.
        unsafe {
            c.setTarget(Some(target));
            c.setAction(Some(sel!(fire:)));
        }
    }

    fn popup(titles: &[&str], target: &Target, tag: isize, mtm: MainThreadMarker) -> Retained<NSPopUpButton> {
        let p = NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), NSRect::ZERO, false);
        for t in titles {
            p.addItemWithTitle(&NSString::from_str(t));
        }
        wire(&p, target, tag);
        p
    }

    fn icon_button(sym: &str, tip: &str, target: &Target, tag: isize, mtm: MainThreadMarker) -> Retained<NSButton> {
        let img = symbol(sym).unwrap_or_default();
        // SAFETY: target/action are wired by `wire` right after.
        let b = unsafe { NSButton::buttonWithImage_target_action(&img, None, None, mtm) };
        // Compact, borderless icons as in Apple's own inspectors.
        b.setBordered(false);
        b.setImagePosition(NSCellImagePosition::ImageOnly);
        b.setToolTip(Some(&NSString::from_str(tip)));
        width(&b, 26.0);
        b.heightAnchor().constraintEqualToConstant(24.0).setActive(true);
        wire(&b, target, tag);
        b
    }

    fn width(v: &NSView, w: f64) {
        v.widthAnchor().constraintEqualToConstant(w).setActive(true);
    }

    fn fill_width(v: &NSView, container: &NSView) {
        v.widthAnchor().constraintEqualToAnchor(&container.widthAnchor()).setActive(true);
    }

    fn menu_item(title: &str, target: &Target, tag: isize, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
        // SAFETY: `fire:` takes one object argument.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(fire:)),
                ns_string!(""),
            )
        };
        item.setTag(tag);
        // SAFETY: target lives as long as the native UI.
        unsafe { item.setTarget(Some(target)) };
        item
    }

    /// Ask for a line of text in a native alert. Returns None on Cancel.
    pub fn prompt_text(title: &str, value: &str) -> Option<String> {
        let mtm = MainThreadMarker::new()?;
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(title));
        alert.addButtonWithTitle(ns_string!("OK"));
        alert.addButtonWithTitle(ns_string!("Cancel"));
        let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
        field.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(240.0, 24.0)));
        alert.setAccessoryView(Some(&field));
        alert.window().setInitialFirstResponder(Some(&field));
        let res = alert.runModal();
        (res == NSAlertFirstButtonReturn).then(|| field.stringValue().to_string())
    }

    // ------------------------------------------------------------ native UI

    #[derive(Default)]
    struct Synced {
        tool: Option<Tool>,
        select_mode: Option<SelectMode>,
        transparent_sel: Option<bool>,
        brush: Option<BrushKind>,
        shape: Option<ShapeKind>,
        outline: Option<PaintStyle>,
        fill: Option<PaintStyle>,
        size: Option<Option<i32>>,
        color1: Option<Color32>,
        color2: Option<Color32>,
        fonts: usize,
        text: Option<crate::text::TextStyle>,
        layers: Option<LayersSig>,
        opacity: Option<i32>,
        undo: Option<(bool, bool)>,
        inspector: Option<bool>,
    }

    #[derive(PartialEq)]
    struct LayersSig {
        rows: Vec<(String, bool, i32)>,
        active: usize,
        revision: u64,
        size: (u32, u32),
    }

    pub struct NativeUi {
        mtm: MainThreadMarker,
        window: Retained<NSWindow>,
        target: Retained<Target>,
        toolbar: Retained<NSToolbar>,
        _toolbar_delegate: Retained<ToolbarDelegate>,
        panel: Retained<NSView>,
        holder: Retained<NSView>,
        rx: Receiver<NativeEvent>,
        saved_style: NSWindowStyleMask,
        saved_toolbar_style: NSWindowToolbarStyle,
        saved_transparent: bool,
        pub inspector_visible: bool,

        tools: Retained<NSSegmentedControl>,
        undo: Retained<NSToolbarItem>,
        redo: Retained<NSToolbarItem>,
        tool_title: Retained<NSTextField>,
        select_box: Retained<NSStackView>,
        select_mode: Retained<NSPopUpButton>,
        transparent_sel: Retained<NSButton>,
        brush_box: Retained<NSStackView>,
        brush: Retained<NSPopUpButton>,
        shape_box: Retained<NSStackView>,
        shape: Retained<NSPopUpButton>,
        outline: Retained<NSPopUpButton>,
        fill: Retained<NSPopUpButton>,
        size_box: Retained<NSStackView>,
        size: Retained<NSSlider>,
        size_label: Retained<NSTextField>,
        text_box: Retained<NSStackView>,
        font: Retained<NSPopUpButton>,
        font_size: Retained<NSPopUpButton>,
        text_style: Retained<NSSegmentedControl>,
        text_bg: Retained<NSPopUpButton>,
        hint: Retained<NSTextField>,
        color1: Retained<NSColorWell>,
        color2: Retained<NSColorWell>,
        layer_rows: Retained<NSStackView>,
        opacity: Retained<NSSlider>,
        opacity_label: Retained<NSTextField>,
        synced: Synced,
    }

    impl NativeUi {
        /// Add the toolbar and inspector to `window`.
        pub fn install(ctx: &egui::Context, window: Retained<NSWindow>) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let (tx, rx) = channel();
            let target = Target::new(mtm, tx, ctx.clone());
            let t: &Target = &target;

            // --- toolbar
            let delegate = ToolbarDelegate::new(mtm);
            let toolbar_item = |id: &str, sym: &str, label_text: &str, tag: isize| {
                let item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), &NSString::from_str(id));
                if let Some(img) = symbol(sym) {
                    item.setImage(Some(&img));
                }
                item.setLabel(&NSString::from_str(label_text));
                item.setToolTip(Some(&NSString::from_str(label_text)));
                item.setTag(tag);
                // We enable/disable items ourselves (e.g. Undo).
                item.setAutovalidates(false);
                // SAFETY: `fire:` takes one object argument.
                unsafe {
                    item.setTarget(Some(t));
                    item.setAction(Some(sel!(fire:)));
                }
                item
            };
            let action_tag = |a: Action| T_ACTION * 1000 + Action::ALL.iter().position(|x| *x == a).unwrap() as isize;
            let undo = toolbar_item("undo", "arrow.uturn.backward", "Undo", action_tag(Action::Undo));
            let redo = toolbar_item("redo", "arrow.uturn.forward", "Redo", action_tag(Action::Redo));

            let tool_symbols = [
                ("rectangle.dashed", "Select"),
                ("pencil", "Pencil"),
                ("drop.halffull", "Fill with color"),
                ("textformat", "Text"),
                ("eraser", "Eraser"),
                ("eyedropper", "Color picker"),
                ("magnifyingglass", "Magnifier"),
                ("paintbrush", "Brushes"),
                ("square.on.circle", "Shapes"),
            ];
            let tools = NSSegmentedControl::new(mtm);
            tools.setSegmentCount(tool_symbols.len() as isize);
            tools.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
            for (i, (sym, tip)) in tool_symbols.iter().enumerate() {
                let i = i as isize;
                match symbol(sym) {
                    Some(img) => tools.setImage_forSegment(Some(&img), i),
                    None => tools.setLabel_forSegment(&NSString::from_str(&tip[..1]), i),
                }
                tools.setToolTip_forSegment(Some(&NSString::from_str(tip)), i);
                tools.setWidth_forSegment(34.0, i);
            }
            wire(&tools, t, T_TOOL * 1000);
            let tools_item = NSToolbarItem::initWithItemIdentifier(NSToolbarItem::alloc(mtm), ns_string!("tools"));
            tools_item.setView(Some(&tools));
            tools_item.setLabel(ns_string!("Tools"));

            let image_menu = NSMenu::new(mtm);
            for a in [Action::Crop, Action::ResizeSkew] {
                image_menu.addItem(&menu_item(a.label(), t, action_tag(a), mtm));
            }
            image_menu.addItem(&NSMenuItem::separatorItem(mtm));
            for a in [Action::RotateRight, Action::RotateLeft, Action::Rotate180, Action::FlipHorizontal, Action::FlipVertical] {
                image_menu.addItem(&menu_item(a.label(), t, action_tag(a), mtm));
            }
            image_menu.addItem(&NSMenuItem::separatorItem(mtm));
            image_menu.addItem(&menu_item(Action::ImageProperties.label(), t, action_tag(Action::ImageProperties), mtm));
            let image_item = NSMenuToolbarItem::initWithItemIdentifier(NSMenuToolbarItem::alloc(mtm), ns_string!("image"));
            image_item.setMenu(&image_menu);
            if let Some(img) = symbol("crop.rotate") {
                image_item.setImage(Some(&img));
            }
            image_item.setLabel(ns_string!("Image"));
            image_item.setToolTip(Some(ns_string!("Crop, resize, rotate and flip")));

            let zoom_out = toolbar_item("zoomOut", "minus.magnifyingglass", "Zoom Out", action_tag(Action::ZoomOut));
            let zoom_in = toolbar_item("zoomIn", "plus.magnifyingglass", "Zoom In", action_tag(Action::ZoomIn));
            let inspector = toolbar_item("inspector", "sidebar.right", "Inspector", T_INSPECTOR * 1000);

            delegate.push(undo.clone());
            delegate.push(redo.clone());
            // SAFETY: AppKit's predefined identifiers are valid static strings.
            unsafe { delegate.push_id(NSToolbarSpaceItemIdentifier) };
            delegate.push(tools_item);
            unsafe { delegate.push_id(NSToolbarFlexibleSpaceItemIdentifier) };
            delegate.push(Retained::into_super(image_item));
            delegate.push(zoom_out);
            delegate.push(zoom_in);
            delegate.push(inspector);

            let toolbar = NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), ns_string!("DabbleToolbar"));
            toolbar.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
            toolbar.setAllowsUserCustomization(false);

            // --- inspector content
            let tool_title = label("Brush", mtm);
            tool_title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));

            let select_mode = popup(&["Rectangle", "Free-form"], t, T_SELECT_MODE * 1000, mtm);
            // SAFETY: wired below.
            let transparent_sel = unsafe {
                NSButton::checkboxWithTitle_target_action(ns_string!("Transparent selection"), None, None, mtm)
            };
            wire(&transparent_sel, t, T_TRANSPARENT_SEL * 1000);
            let sel_buttons = stack(
                false,
                6.0,
                &[
                    &icon_button("checkmark.rectangle", "Select All", t, action_tag(Action::SelectAll), mtm),
                    &icon_button("crop", "Crop to selection", t, action_tag(Action::Crop), mtm),
                    &icon_button("trash", "Delete selection", t, action_tag(Action::Delete), mtm),
                    &icon_button("arrow.up.left.and.arrow.down.right", "Resize and skew", t, action_tag(Action::ResizeSkew), mtm),
                ],
                mtm,
            );
            let select_box = stack(true, 8.0, &[&select_mode, &transparent_sel, &sel_buttons], mtm);

            let brush_names: Vec<&str> = BrushKind::ALL.iter().map(|k| k.name()).collect();
            let brush = popup(&brush_names, t, T_BRUSH * 1000, mtm);
            let brush_box = stack(true, 8.0, &[&brush], mtm);

            let shape_names: Vec<&str> =
                ShapeKind::ALL.iter().map(|k| k.name().split(" (").next().unwrap_or("")).collect();
            let shape = popup(&shape_names, t, T_SHAPE * 1000, mtm);
            let style_names: Vec<&str> = PaintStyle::ALL.iter().map(|s| s.name()).collect();
            let outline = popup(&style_names, t, T_OUTLINE * 1000, mtm);
            let fill = popup(&style_names, t, T_FILL * 1000, mtm);
            let outline_row = stack(false, 8.0, &[&label("Outline", mtm), &outline], mtm);
            let fill_row = stack(false, 8.0, &[&label("Fill", mtm), &fill], mtm);
            let shape_box = stack(true, 8.0, &[&shape, &outline_row, &fill_row], mtm);

            // SAFETY: wired below.
            let size = unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(5.0, 1.0, 100.0, None, None, mtm) };
            wire(&size, t, T_SIZE * 1000);
            width(&size, 150.0);
            let size_label = label("5 px", mtm);
            let size_box = stack(false, 8.0, &[&label("Size", mtm), &size, &size_label], mtm);

            let font = popup(&[], t, T_FONT * 1000, mtm);
            width(&font, 200.0);
            let size_titles: Vec<String> = ribbon::FONT_SIZES.iter().map(|s| format!("{s} pt")).collect();
            let size_refs: Vec<&str> = size_titles.iter().map(|s| s.as_str()).collect();
            let font_size = popup(&size_refs, t, T_FONT_SIZE * 1000, mtm);
            let text_style = NSSegmentedControl::new(mtm);
            text_style.setSegmentCount(4);
            text_style.setTrackingMode(NSSegmentSwitchTracking::SelectAny);
            for (i, (sym, tip)) in
                [("bold", "Bold"), ("italic", "Italic"), ("underline", "Underline"), ("strikethrough", "Strikethrough")]
                    .iter()
                    .enumerate()
            {
                match symbol(sym) {
                    Some(img) => text_style.setImage_forSegment(Some(&img), i as isize),
                    None => text_style.setLabel_forSegment(&NSString::from_str(&tip[..1]), i as isize),
                }
                text_style.setToolTip_forSegment(Some(&NSString::from_str(tip)), i as isize);
            }
            wire(&text_style, t, T_TEXT_STYLE * 1000);
            let text_bg = popup(&["Transparent background", "Fill background (Color 2)"], t, T_TEXT_BG * 1000, mtm);
            let text_row = stack(false, 8.0, &[&font_size, &text_style], mtm);
            let text_box = stack(true, 8.0, &[&font, &text_row, &text_bg], mtm);

            let hint_label = hint("", mtm);

            // Colors.
            let color1 = NSColorWell::colorWellWithStyle(NSColorWellStyle::Default, mtm);
            let color2 = NSColorWell::colorWellWithStyle(NSColorWellStyle::Default, mtm);
            wire(&color1, t, T_COLOR1 * 1000);
            wire(&color2, t, T_COLOR2 * 1000);
            for w in [&color1, &color2] {
                width(w, 44.0);
            }
            let wells = stack(
                false,
                8.0,
                &[&color1, &label("Color 1", mtm), &color2, &label("Color 2", mtm)],
                mtm,
            );
            let mut palette_rows = vec![];
            for row in 0..2 {
                let buttons: Vec<Retained<NSButton>> = (0..10)
                    .map(|i| {
                        let idx = row * 10 + i;
                        let c = ribbon::PALETTE[idx];
                        let img = swatch_image(Color32::from_rgb(c[0], c[1], c[2])).unwrap_or_default();
                        // SAFETY: wired below.
                        let b = unsafe { NSButton::buttonWithImage_target_action(&img, None, None, mtm) };
                        b.setBordered(false);
                        b.setToolTip(Some(ns_string!("Click: Color 1 · Option-click: Color 2")));
                        wire(&b, t, T_PALETTE * 1000 + idx as isize);
                        b
                    })
                    .collect();
                let refs: Vec<&NSView> = buttons.iter().map(|b| &***b as &NSView).collect();
                palette_rows.push(stack(false, 4.0, &refs, mtm));
            }

            // Layers.
            let layer_ops = stack(
                false,
                6.0,
                &[
                    &icon_button("plus", "New layer", t, T_LAYER_OP * 1000, mtm),
                    &icon_button("plus.square.on.square", "Duplicate layer", t, T_LAYER_OP * 1000 + 1, mtm),
                    &icon_button("trash", "Delete layer", t, T_LAYER_OP * 1000 + 2, mtm),
                    &icon_button("arrow.up", "Move layer up", t, T_LAYER_OP * 1000 + 3, mtm),
                    &icon_button("arrow.down", "Move layer down", t, T_LAYER_OP * 1000 + 4, mtm),
                    &icon_button("square.stack.3d.down.right", "Merge down", t, T_LAYER_OP * 1000 + 5, mtm),
                    &icon_button("pencil.line", "Rename layer", t, T_LAYER_OP * 1000 + 6, mtm),
                ],
                mtm,
            );
            let layer_rows = stack(true, 2.0, &[], mtm);
            // SAFETY: wired below.
            let opacity =
                unsafe { NSSlider::sliderWithValue_minValue_maxValue_target_action(100.0, 0.0, 100.0, None, None, mtm) };
            wire(&opacity, t, T_LAYER_OPACITY * 1000);
            width(&opacity, 140.0);
            let opacity_label = label("100%", mtm);
            let opacity_row = stack(false, 8.0, &[&label("Opacity", mtm), &opacity, &opacity_label], mtm);

            let content = stack(
                true,
                10.0,
                &[
                    &heading("TOOL", mtm),
                    &tool_title,
                    &select_box,
                    &brush_box,
                    &shape_box,
                    &text_box,
                    &size_box,
                    &hint_label,
                    &heading("COLORS", mtm),
                    &wells,
                    &palette_rows[0],
                    &palette_rows[1],
                    &heading("LAYERS", mtm),
                    &layer_ops,
                    &layer_rows,
                    &opacity_row,
                ],
                mtm,
            );
            content.setCustomSpacing_afterView(16.0, &hint_label);
            content.setCustomSpacing_afterView(16.0, &palette_rows[1]);
            content.setTranslatesAutoresizingMaskIntoConstraints(false);

            let holder = NSView::new(mtm);
            holder.addSubview(&content);
            content.topAnchor().constraintEqualToAnchor_constant(&holder.topAnchor(), 16.0).setActive(true);
            content.leadingAnchor().constraintEqualToAnchor_constant(&holder.leadingAnchor(), 16.0).setActive(true);
            content.trailingAnchor().constraintEqualToAnchor_constant(&holder.trailingAnchor(), -16.0).setActive(true);
            fill_width(&layer_rows, &content);

            // Liquid Glass on macOS 26, a sidebar material before that.
            let panel: Retained<NSView> = if objc2::runtime::AnyClass::get(c"NSGlassEffectView").is_some() {
                let g = NSGlassEffectView::new(mtm);
                g.setCornerRadius(18.0);
                g.setContentView(Some(&holder));
                Retained::into_super(g)
            } else {
                let v = NSVisualEffectView::new(mtm);
                v.setMaterial(NSVisualEffectMaterial::Sidebar);
                v.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
                v.setState(NSVisualEffectState::Active);
                v.addSubview(&holder);
                holder.setTranslatesAutoresizingMaskIntoConstraints(false);
                for (a, b) in [(holder.topAnchor(), v.topAnchor()), (holder.bottomAnchor(), v.bottomAnchor())] {
                    a.constraintEqualToAnchor(&b).setActive(true);
                }
                holder.leadingAnchor().constraintEqualToAnchor(&v.leadingAnchor()).setActive(true);
                holder.trailingAnchor().constraintEqualToAnchor(&v.trailingAnchor()).setActive(true);
                Retained::into_super(v)
            };

            let mut ui = Self {
                mtm,
                saved_style: window.styleMask(),
                saved_toolbar_style: window.toolbarStyle(),
                saved_transparent: window.titlebarAppearsTransparent(),
                window,
                target,
                toolbar,
                _toolbar_delegate: delegate,
                panel,
                holder: holder.clone(),
                rx,
                inspector_visible: true,
                tools,
                undo,
                redo,
                tool_title,
                select_box,
                select_mode,
                transparent_sel,
                brush_box,
                brush,
                shape_box,
                shape,
                outline,
                fill,
                size_box,
                size,
                size_label,
                text_box,
                font,
                font_size,
                text_style,
                text_bg,
                hint: hint_label,
                color1,
                color2,
                layer_rows,
                opacity,
                opacity_label,
                synced: Synced::default(),
            };
            ui.attach();
            Some(ui)
        }

        fn attach(&mut self) {
            let w = &self.window;
            w.setStyleMask(self.saved_style | NSWindowStyleMask::FullSizeContentView);
            w.setTitlebarAppearsTransparent(true);
            w.setToolbarStyle(NSWindowToolbarStyle::Unified);
            w.setToolbar(Some(&self.toolbar));
            let Some(view) = w.contentView() else { return };
            let p = &self.panel;
            p.setTranslatesAutoresizingMaskIntoConstraints(false);
            view.addSubview(p);
            let guide = view.safeAreaLayoutGuide();
            p.topAnchor().constraintEqualToAnchor_constant(&guide.topAnchor(), PANEL_MARGIN).setActive(true);
            p.bottomAnchor()
                .constraintEqualToAnchor_constant(&view.bottomAnchor(), -(STATUS_BAR + PANEL_MARGIN))
                .setActive(true);
            p.trailingAnchor().constraintEqualToAnchor_constant(&view.trailingAnchor(), -PANEL_MARGIN).setActive(true);
            p.widthAnchor().constraintEqualToConstant(PANEL_WIDTH).setActive(true);
            p.setHidden(!self.inspector_visible);
        }

        /// Remove the toolbar and inspector and restore the window.
        pub fn uninstall(self) {
            self.panel.removeFromSuperview();
            self.window.setToolbar(None);
            self.window.setToolbarStyle(self.saved_toolbar_style);
            self.window.setTitlebarAppearsTransparent(self.saved_transparent);
            self.window.setStyleMask(self.saved_style);
        }

        pub fn drain(&self) -> Vec<NativeEvent> {
            let events: Vec<NativeEvent> = self.rx.try_iter().collect();
            if !events.is_empty() {
                // Hand the keyboard back to the canvas after a click on a control.
                if let Some(view) = self.window.contentView() {
                    self.window.makeFirstResponder(Some(&view));
                }
            }
            events
        }

        pub fn set_inspector_visible(&mut self, on: bool) {
            self.inspector_visible = on;
            self.panel.setHidden(!on);
        }

        /// Space the toolbar and inspector cover, in points: (top, right).
        pub fn insets(&self) -> (f32, f32) {
            let frame = self.window.frame();
            let layout = self.window.contentLayoutRect();
            let top = (frame.size.height - layout.size.height).max(0.0) as f32;
            let right = if self.inspector_visible { (PANEL_WIDTH + 2.0 * PANEL_MARGIN) as f32 } else { 0.0 };
            (top, right)
        }

        pub fn set_appearance(&self, dark: Option<bool>) {
            let app = NSApplication::sharedApplication(self.mtm);
            let appearance = dark.and_then(|d| {
                // SAFETY: AppKit's appearance names are valid static strings.
                let name = unsafe { if d { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
                NSAppearance::appearanceNamed(name)
            });
            app.setAppearance(appearance.as_deref());
        }

        fn rebuild_layers(&self, app: &DabbleApp) {
            let mtm = self.mtm;
            let t: &Target = &self.target;
            for v in self.layer_rows.arrangedSubviews().iter() {
                self.layer_rows.removeArrangedSubview(&v);
                v.removeFromSuperview();
            }
            let (w, h) = (app.doc.width() as f32, app.doc.height() as f32);
            let scale = (40.0 / w).min(30.0 / h);
            let (tw, th) = ((w * scale).max(1.0).round() as u32, (h * scale).max(1.0).round() as u32);
            for i in (0..app.layers.len()).rev() {
                let l = &app.layers[i];
                let active = i == app.active;
                let pm = if active { &app.doc } else { &l.pm };
                // Thumbnail on white so transparent layers still read.
                let thumb = raster::scale(pm, tw * 2, th * 2).map(|s| {
                    let mut bg = raster::new_canvas(tw * 2, th * 2, Color32::from_gray(245));
                    raster::draw_over(&mut bg, &s, 0, 0);
                    bg
                });
                let img = thumb.and_then(|p| png_image(&p, (tw as f64, th as f64))).unwrap_or_default();
                // SAFETY: wired below.
                let thumb_btn = unsafe { NSButton::buttonWithImage_target_action(&img, None, None, mtm) };
                thumb_btn.setBordered(false);
                wire(&thumb_btn, t, T_LAYER_SELECT * 1000 + i as isize);
                width(&thumb_btn, 44.0);

                // SAFETY: wired below.
                let name = unsafe {
                    NSButton::buttonWithTitle_target_action(&NSString::from_str(&l.name), None, None, mtm)
                };
                name.setBordered(false);
                name.setAlignment(NSTextAlignment::Left);
                if active {
                    name.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
                }
                wire(&name, t, T_LAYER_SELECT * 1000 + i as isize);

                let pct = label(&format!("{:.0}%", l.opacity * 100.0), mtm);
                pct.setTextColor(Some(&NSColor::secondaryLabelColor()));
                pct.setFont(Some(&NSFont::systemFontOfSize(11.0)));
                let eye = icon_button(
                    if l.visible { "eye" } else { "eye.slash" },
                    if l.visible { "Hide layer" } else { "Show layer" },
                    t,
                    T_LAYER_EYE * 1000 + i as isize,
                    mtm,
                );
                let spacer = NSView::new(mtm);
                spacer.setContentHuggingPriority_forOrientation(1.0, NSLayoutConstraintOrientation::Horizontal);
                let row = stack(false, 8.0, &[&thumb_btn, &name, &spacer, &pct, &eye], mtm);

                let bx = NSBox::new(mtm);
                bx.setBoxType(NSBoxType::Custom);
                bx.setTitlePosition(NSTitlePosition::NoTitle);
                bx.setBorderWidth(0.0);
                bx.setCornerRadius(8.0);
                bx.setContentViewMargins(NSSize::new(6.0, 4.0));
                let fill = if active {
                    NSColor::selectedContentBackgroundColor().colorWithAlphaComponent(0.25)
                } else {
                    NSColor::clearColor()
                };
                bx.setFillColor(&fill);
                bx.setContentView(Some(&row));
                self.layer_rows.addArrangedSubview(&bx);
                fill_width(&bx, &self.layer_rows);
            }
        }
    }

    impl DabbleApp {
        /// Push app state into the native controls where it changed.
        pub fn sync_native(&mut self) {
            let Some(mut ui) = self.native.take() else { return };
            let s = &mut ui.synced;

            if s.tool != Some(self.tool) {
                s.tool = Some(self.tool);
                if let Some(i) = TOOLS.iter().position(|t| *t == self.tool) {
                    ui.tools.setSelectedSegment(i as isize);
                }
                let (title, hint_text) = match self.tool {
                    Tool::Select => ("Selection", ""),
                    Tool::Pencil => ("Pencil", ""),
                    Tool::Fill => ("Fill", "Click an area to fill it with Color 1. Right-click uses Color 2."),
                    Tool::Text => ("Text", "Click or drag on the canvas to add a text box. Esc finishes it."),
                    Tool::Eraser => ("Eraser", "Right-drag replaces only Color 1 with Color 2."),
                    Tool::Picker => ("Color picker", "Click to pick Color 1. Right-click picks Color 2."),
                    Tool::Magnifier => ("Magnifier", "Click to zoom in. Right-click zooms out."),
                    Tool::Brush => ("Brushes", ""),
                    Tool::Shape => ("Shapes", "Shapes stay editable until you click away or press Enter."),
                };
                ui.tool_title.setStringValue(&NSString::from_str(title));
                ui.hint.setStringValue(&NSString::from_str(hint_text));
                ui.hint.setHidden(hint_text.is_empty());
                ui.select_box.setHidden(self.tool != Tool::Select);
                ui.brush_box.setHidden(self.tool != Tool::Brush);
                ui.shape_box.setHidden(self.tool != Tool::Shape);
                ui.text_box.setHidden(self.tool != Tool::Text);
                s.size = None;
            }
            if s.select_mode != Some(self.select_mode) {
                s.select_mode = Some(self.select_mode);
                ui.select_mode.selectItemAtIndex(if self.select_mode == SelectMode::Rect { 0 } else { 1 });
            }
            if s.transparent_sel != Some(self.transparent_selection) {
                s.transparent_sel = Some(self.transparent_selection);
                ui.transparent_sel
                    .setState(if self.transparent_selection { NSControlStateValueOn } else { NSControlStateValueOff });
            }
            if s.brush != Some(self.brush_kind) {
                s.brush = Some(self.brush_kind);
                let i = BrushKind::ALL.iter().position(|k| *k == self.brush_kind).unwrap_or(0);
                ui.brush.selectItemAtIndex(i as isize);
            }
            if s.shape != Some(self.shape_kind) {
                s.shape = Some(self.shape_kind);
                let i = ShapeKind::ALL.iter().position(|k| *k == self.shape_kind).unwrap_or(0);
                ui.shape.selectItemAtIndex(i as isize);
            }
            if s.outline != Some(self.shape_outline) {
                s.outline = Some(self.shape_outline);
                let i = PaintStyle::ALL.iter().position(|k| *k == self.shape_outline).unwrap_or(0);
                ui.outline.selectItemAtIndex(i as isize);
            }
            if s.fill != Some(self.shape_fill) {
                s.fill = Some(self.shape_fill);
                let i = PaintStyle::ALL.iter().position(|k| *k == self.shape_fill).unwrap_or(0);
                ui.fill.selectItemAtIndex(i as isize);
            }
            let size = self.size_for_tool().map(|v| v.round() as i32);
            let s = &mut ui.synced;
            if s.size != Some(size) {
                s.size = Some(size);
                ui.size_box.setHidden(size.is_none());
                if let Some(v) = size {
                    ui.size.setDoubleValue(v as f64);
                    ui.size_label.setStringValue(&NSString::from_str(&format!("{v} px")));
                }
            }
            if s.color1 != Some(self.primary) {
                s.color1 = Some(self.primary);
                ui.color1.setColor(&nscolor(self.primary));
            }
            if s.color2 != Some(self.secondary) {
                s.color2 = Some(self.secondary);
                ui.color2.setColor(&nscolor(self.secondary));
            }

            // Text options: the font list fills in once fonts are loaded.
            let families = self.fonts.as_ref().map(|f| f.families.clone()).unwrap_or_default();
            if s.fonts != families.len() {
                s.fonts = families.len();
                ui.font.removeAllItems();
                for f in &families {
                    ui.font.addItemWithTitle(&NSString::from_str(f));
                }
                s.text = None;
            }
            if s.text.as_ref() != Some(&self.text_style) {
                let st = self.text_style.clone();
                if let Some(i) = families.iter().position(|f| *f == st.family) {
                    ui.font.selectItemAtIndex(i as isize);
                }
                if let Some(i) = ribbon::FONT_SIZES.iter().position(|v| *v == st.size_pt) {
                    ui.font_size.selectItemAtIndex(i as isize);
                }
                for (i, on) in [st.bold, st.italic, st.underline, st.strike].into_iter().enumerate() {
                    ui.text_style.setSelected_forSegment(on, i as isize);
                }
                ui.text_bg.selectItemAtIndex(if st.opaque_bg { 1 } else { 0 });
                ui.synced.text = Some(st);
            }

            let can = (self.history.can_undo() || self.pending.is_some(), self.history.can_redo() && self.pending.is_none());
            if ui.synced.undo != Some(can) {
                ui.synced.undo = Some(can);
                ui.undo.setEnabled(can.0);
                ui.redo.setEnabled(can.1);
            }

            let sig = LayersSig {
                rows: self.layers.iter().map(|l| (l.name.clone(), l.visible, (l.opacity * 100.0).round() as i32)).collect(),
                active: self.active,
                revision: self.history.revision(),
                size: (self.doc.width(), self.doc.height()),
            };
            if ui.synced.layers.as_ref() != Some(&sig) {
                ui.rebuild_layers(self);
                ui.synced.layers = Some(sig);
            }
            let op = (self.layers[self.active].opacity * 100.0).round() as i32;
            if ui.synced.opacity != Some(op) {
                ui.synced.opacity = Some(op);
                ui.opacity.setDoubleValue(op as f64);
                ui.opacity_label.setStringValue(&NSString::from_str(&format!("{op}%")));
            }
            if ui.synced.inspector != Some(ui.inspector_visible) {
                ui.synced.inspector = Some(ui.inspector_visible);
                ui.panel.setHidden(!ui.inspector_visible);
            }
            self.native = Some(ui);
        }
    }

    /// The window that `view` belongs to.
    pub fn window_of(view: std::ptr::NonNull<std::ffi::c_void>) -> Option<Retained<NSWindow>> {
        // SAFETY: raw-window-handle gives a valid NSView pointer on macOS.
        let view: &NSView = unsafe { view.cast().as_ref() };
        view.window()
    }

    impl NativeUi {
        /// Debug aid: also save the inspector's controls (glass hides them from `snapshot`).
        pub fn snapshot_inspector(&self, path: &std::path::Path) -> bool {
            view_png(&self.holder, path)
        }
    }

    /// Debug aid: save what AppKit draws of the window (toolbar, inspector) to a PNG.
    /// The Metal canvas and glass blur don't show up in this capture.
    pub fn snapshot(window: &NSWindow, path: &std::path::Path) -> bool {
        let Some(view) = window.contentView().and_then(|v| unsafe { v.superview() }) else { return false };
        view_png(&view, path)
    }

    fn view_png(view: &NSView, path: &std::path::Path) -> bool {
        let bounds = view.bounds();
        let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) else { return false };
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        // SAFETY: an empty properties dictionary is valid.
        let data = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new())
        };
        data.is_some_and(|d| std::fs::write(path, d.to_vec()).is_ok())
    }
}
