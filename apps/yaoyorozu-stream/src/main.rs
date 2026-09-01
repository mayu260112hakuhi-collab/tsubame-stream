use yaoyorozu_stream::app::YaoyorozuApp;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("燕 / Tsubame")
            .with_inner_size([1280.0, 900.0])
            .with_min_inner_size([960.0, 720.0]),
        ..Default::default()
    };

    eframe::run_native(
        "燕 / Tsubame",
        options,
        Box::new(|cc| Ok(Box::new(YaoyorozuApp::new(cc)))),
    )
}
