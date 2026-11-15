mod app;
mod theme;
mod ui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mfalmion")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([760.0, 540.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eprintln!(
        "level=INFO event=startup application=Mfalmion version={}",
        env!("CARGO_PKG_VERSION")
    );
    let result = eframe::run_native(
        "Mfalmion",
        options,
        Box::new(|cc| Ok(Box::new(app::MfalmionApp::new(cc)))),
    );
    if let Err(error) = &result {
        eprintln!("level=ERROR event=desktop_failed error={error:?}");
    } else {
        eprintln!("level=INFO event=shutdown");
    }
    result
}
