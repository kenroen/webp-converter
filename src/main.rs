use webp_converter::{app, cli};

fn main() -> eframe::Result {
    if std::env::args_os().len() > 1 {
        std::process::exit(cli::run_from_args(std::env::args_os()));
    }

    let viewport = egui::ViewportBuilder::default()
        .with_title("WEBP // Converter")
        .with_app_id("webp-converter")
        .with_inner_size([app::WINDOW_W, app::WINDOW_H])
        .with_min_inner_size([app::WINDOW_W, app::WINDOW_H])
        .with_max_inner_size([app::WINDOW_W, app::WINDOW_H])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_decorations(false);

    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "WEBP // Converter",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
