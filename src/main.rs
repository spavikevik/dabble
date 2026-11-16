mod app;
mod brushes;
mod canvas;
mod dialogs;
mod history;
mod io;
mod layers;
mod mac_ui;
mod menu;
mod raster;
mod ribbon;
mod settings;
mod shapes;
mod text;
mod theme;
#[cfg(test)]
mod tests;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // eframe names the macOS app menu after the first title, so start
            // with the app name; the window title updates on the first frame.
            .with_title("Dabble")
            // An empty icon tells eframe to keep the .app bundle's icon.
            .with_icon(egui::IconData::default())
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Dabble",
        options,
        Box::new(move |cc| Ok(Box::new(app::DabbleApp::new(cc, initial)))),
    )
}
