/// Dark / Light theme support.
use eframe::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
  Dark,
  Light,
}

impl Theme {
  pub fn toggle(&mut self) {
    *self = match self {
      Theme::Dark => Theme::Light,
      Theme::Light => Theme::Dark,
    };
  }

  pub fn label(&self) -> &'static str {
    match self {
      Theme::Dark => "🌙 Dark",
      Theme::Light => "☀️ Light",
    }
  }

  pub fn apply(&self, ctx: &egui::Context) {
    match self {
      Theme::Dark => ctx.set_visuals(egui::Visuals::dark()),
      Theme::Light => ctx.set_visuals(egui::Visuals::light()),
    }
  }
}

impl Default for Theme {
  fn default() -> Self {
    Theme::Dark
  }
}
