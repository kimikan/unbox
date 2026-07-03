mod app;
mod converter;
mod theme;

use eframe::egui;

#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() -> eframe::Result<()> {
  env_logger::init();

  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_inner_size([720.0, 480.0])
      .with_min_inner_size([520.0, 360.0]),
    ..Default::default()
  };

  eframe::run_native(
    "Unbox DSSAD Data Converter",
    options,
    Box::new(|cc| Ok(Box::new(app::UnboxApp::new(cc)))),
  )
}
