mod app;
mod canvas;
mod dialogs;
mod history;
mod io;
mod menu;
mod raster;
mod ribbon;
mod shapes;
mod text;
mod theme;
#[cfg(test)]
mod tests;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Untitled - Daub")
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([720.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Daub",
        options,
        Box::new(move |cc| Ok(Box::new(app::DaubApp::new(cc, initial)))),
    )
}
