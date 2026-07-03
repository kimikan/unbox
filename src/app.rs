use std::sync::{Arc, Mutex};

use eframe::egui;
use egui_file_dialog::FileDialog;

use crate::converter::{self, ConvertResult};
use crate::theme::Theme;

pub struct UnboxApp {
  theme: Theme,
  file_dialog: FileDialog,
  selected_dir: Option<std::path::PathBuf>,

  // Async conversion state
  shared_results: Arc<Mutex<Vec<ConvertResult>>>,
  displayed_results: Vec<ConvertResult>,
  total_files: usize,
  is_running: bool,
  status: String,
}

impl UnboxApp {
  pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
    let theme = Theme::default();
    theme.apply(&cc.egui_ctx);
    apply_custom_style(&cc.egui_ctx);

    Self {
      theme,
      file_dialog: FileDialog::new(),
      selected_dir: None,
      shared_results: Arc::new(Mutex::new(Vec::new())),
      displayed_results: Vec::new(),
      total_files: 0,
      is_running: false,
      status: "Ready".into(),
    }
  }

  fn start_conversion(&mut self, ctx: &egui::Context) {
    let Some(dir) = self.selected_dir.clone() else {
      return;
    };

    self.total_files = converter::count_files(&dir);
    if self.total_files == 0 {
      self.status = "No convertible files found.".into();
      return;
    }

    self.is_running = true;
    self.displayed_results.clear();
    let results = Arc::new(Mutex::new(Vec::new()));
    self.shared_results = results.clone();
    self.status = format!("Converting 0/{} ...", self.total_files);

    let ctx = ctx.clone();
    std::thread::spawn(move || {
      converter::convert_directory_incremental(&dir, &results);
      ctx.request_repaint();
    });
  }

  fn poll_results(&mut self) {
    if !self.is_running {
      return;
    }

    let results = self.shared_results.lock().unwrap();
    let done = results.len();
    self.displayed_results = results.clone();
    drop(results);

    if done >= self.total_files {
      self.is_running = false;
      let ok = self.displayed_results.iter().filter(|r| r.ok).count();
      let fail = done - ok;
      self.status = format!("Done — {ok} succeeded, {fail} failed, {done} total");
    } else {
      self.status = format!("Converting {done}/{} ...", self.total_files);
    }
  }
}

fn apply_custom_style(ctx: &egui::Context) {
  let mut style = (*ctx.style()).clone();
  style.spacing.item_spacing = egui::vec2(10.0, 8.0);
  style.spacing.button_padding = egui::vec2(12.0, 6.0);

  let rounding = egui::CornerRadius::same(8);
  style.visuals.widgets.inactive.corner_radius = rounding;
  style.visuals.widgets.hovered.corner_radius = rounding;
  style.visuals.widgets.active.corner_radius = rounding;
  style.visuals.widgets.noninteractive.corner_radius = rounding;

  ctx.set_style(style);
}

impl eframe::App for UnboxApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    self.poll_results();

    if self.is_running {
      ctx.request_repaint();
    }

    // ── Top bar ──
    egui::TopBottomPanel::top("toolbar")
      .frame(
        egui::Frame::NONE
          .inner_margin(egui::Margin::symmetric(16, 10))
          .fill(ctx.style().visuals.panel_fill),
      )
      .show(ctx, |ui| {
        ui.horizontal(|ui| {
          ui.label(egui::RichText::new("Unbox").size(20.0).strong());

          ui.add_space(12.0);

          let btn = egui::Button::new("Open Directory");
          if ui.add_enabled(!self.is_running, btn).clicked() {
            self.file_dialog.pick_directory();
          }

          if let Some(dir) = &self.selected_dir {
            ui.add_space(8.0);
            let path_str = dir.display().to_string();
            let display = if path_str.len() > 50 {
              format!("...{}", &path_str[path_str.len() - 47..])
            } else {
              path_str
            };
            ui.label(
              egui::RichText::new(display)
                .monospace()
                .color(ui.visuals().weak_text_color()),
            );
          }

          ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(self.theme.label()).clicked() {
              self.theme.toggle();
              self.theme.apply(ctx);
            }
          });
        });
      });

    // ── Bottom status bar ──
    egui::TopBottomPanel::bottom("status_bar")
      .frame(
        egui::Frame::NONE
          .inner_margin(egui::Margin::symmetric(16, 8))
          .fill(ctx.style().visuals.panel_fill),
      )
      .show(ctx, |ui| {
        ui.horizontal(|ui| {
          if self.is_running {
            ui.spinner();
          }
          ui.label(
            egui::RichText::new(&self.status)
              .size(12.0)
              .color(ui.visuals().weak_text_color()),
          );

          if !self.displayed_results.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
              // CRC stats pill
              let (crc_total, crc_passed, crc_failed) = crc_totals(&self.displayed_results);
              if crc_total > 0 {
                let crc_color = if crc_failed == 0 {
                  egui::Color32::from_rgb(76, 175, 80)
                } else {
                  egui::Color32::from_rgb(244, 67, 54)
                };
                let crc_pill = egui::Frame::NONE
                  .inner_margin(egui::Margin::symmetric(10, 3))
                  .corner_radius(egui::CornerRadius::same(12))
                  .fill(crc_color.gamma_multiply(0.15));
                crc_pill.show(ui, |ui| {
                  ui.label(
                    egui::RichText::new(format!("CRC {crc_passed}/{crc_total}"))
                      .size(11.0)
                      .color(crc_color),
                  );
                });
                ui.add_space(6.0);
              }

              // File conversion stats pill
              let ok = self.displayed_results.iter().filter(|r| r.ok).count();
              let total = self.displayed_results.len();
              let color = if ok == total {
                egui::Color32::from_rgb(76, 175, 80)
              } else {
                egui::Color32::from_rgb(255, 167, 38)
              };

              let pill = egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 3))
                .corner_radius(egui::CornerRadius::same(12))
                .fill(color.gamma_multiply(0.15));

              pill.show(ui, |ui| {
                ui.label(
                  egui::RichText::new(format!("{ok}/{total} OK"))
                    .size(11.0)
                    .color(color),
                );
              });
            });
          }
        });
      });

    // ── Central panel ──
    egui::CentralPanel::default()
      .frame(
        egui::Frame::NONE
          .inner_margin(egui::Margin::symmetric(20, 16))
          .fill(ctx.style().visuals.panel_fill),
      )
      .show(ctx, |ui| {
        // Progress bar while running
        if self.is_running && self.total_files > 0 {
          let done = self.displayed_results.len();
          let progress = done as f32 / self.total_files as f32;
          ui.add(
            egui::ProgressBar::new(progress)
              .text(format!("{done}/{}", self.total_files))
              .animate(true),
          );
          ui.add_space(12.0);
        }

        if self.displayed_results.is_empty() && !self.is_running {
          ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 3.0);
            ui.label(
              egui::RichText::new("Select a directory to start conversion")
                .size(16.0)
                .color(ui.visuals().weak_text_color()),
            );
          });
        } else {
          egui::ScrollArea::vertical().show(ui, |ui| {
            for r in &self.displayed_results {
              render_result_card(ui, r);
              ui.add_space(4.0);
            }
          });
        }
      });

    // ── File dialog ──
    self.file_dialog.update(ctx);
    if let Some(path) = self.file_dialog.take_picked() {
      self.selected_dir = Some(path);
      self.start_conversion(ctx);
    }
  }
}

fn render_result_card(ui: &mut egui::Ui, r: &ConvertResult) {
  let (dot_color, bg_tint) = if r.ok {
    (
      egui::Color32::from_rgb(76, 175, 80),
      egui::Color32::from_rgb(76, 175, 80),
    )
  } else {
    (
      egui::Color32::from_rgb(244, 67, 54),
      egui::Color32::from_rgb(244, 67, 54),
    )
  };

  let card = egui::Frame::NONE
    .inner_margin(egui::Margin::symmetric(12, 8))
    .corner_radius(egui::CornerRadius::same(6))
    .fill(bg_tint.gamma_multiply(0.06))
    .stroke(egui::Stroke::new(0.5_f32, bg_tint.gamma_multiply(0.2)));

  card.show(ui, |ui| {
    ui.horizontal(|ui| {
      // Status dot
      let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
      ui.painter().circle_filled(rect.center(), 5.0, dot_color);

      ui.add_space(8.0);

      // File names
      ui.vertical(|ui| {
        ui.label(
          egui::RichText::new(
            r.src
              .file_name()
              .unwrap_or_default()
              .to_string_lossy()
              .to_string(),
          )
          .strong(),
        );
        let dst_name = r
          .dst
          .file_name()
          .unwrap_or_default()
          .to_string_lossy()
          .to_string();
        if !dst_name.is_empty() {
          ui.label(
            egui::RichText::new(format!("-> {dst_name}"))
              .size(11.0)
              .color(ui.visuals().weak_text_color()),
          );
        }
      });

      ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // CRC badge (if applicable)
        if let Some(stats) = &r.crc_stats {
          let crc_color = if stats.failed == 0 {
            egui::Color32::from_rgb(76, 175, 80)
          } else {
            egui::Color32::from_rgb(244, 67, 54)
          };
          let badge = egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(6, 2))
            .corner_radius(egui::CornerRadius::same(10))
            .fill(crc_color.gamma_multiply(0.12));
          badge.show(ui, |ui| {
            ui.label(
              egui::RichText::new(format!("CRC {}/{}", stats.passed, stats.total))
                .size(10.0)
                .color(crc_color),
            );
          });
          ui.add_space(6.0);
        }

        ui.label(
          egui::RichText::new(&r.message)
            .size(12.0)
            .color(ui.visuals().text_color()),
        );
      });
    });
  });
}

fn crc_totals(results: &[ConvertResult]) -> (usize, usize, usize) {
  let mut total = 0;
  let mut passed = 0;
  let mut failed = 0;
  for r in results {
    if let Some(stats) = &r.crc_stats {
      total += stats.total;
      passed += stats.passed;
      failed += stats.failed;
    }
  }
  (total, passed, failed)
}
