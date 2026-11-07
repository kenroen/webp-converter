//! Desktop GUI for WebP Converter.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, vec2,
};
use serde::{Deserialize, Serialize};

use crate::chrome::{self, WindowButton};
use crate::config::{AnimatedPolicy, ConversionConfig, ExistingPolicy, TargetFormat};
use crate::scanner::{DiscoveredFile, scan_inputs};
use crate::theme;
use crate::worker::{BatchSummary, WorkerEvent, spawn_batch};

pub const WINDOW_W: f32 = 640.0;
pub const WINDOW_H: f32 = 780.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemStatus {
    Ready,
    Working,
    Done {
        outputs: Vec<PathBuf>,
        was_animated: bool,
        duration_ms: u64,
    },
    Skipped {
        reason: String,
    },
    Failed {
        error: String,
    },
}

#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub file: DiscoveredFile,
    pub status: ItemStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedState {
    config: ConversionConfig,
    on_top: bool,
}

#[derive(Default)]
pub struct App {
    s: SavedState,
    applied_on_top: Option<bool>,
    queue: Vec<QueueEntry>,
    rx: Option<Receiver<WorkerEvent>>,
    cancel: Option<Arc<AtomicBool>>,
    last_summary: Option<BatchSummary>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let s: SavedState = cc
            .storage
            .and_then(|st| eframe::get_value(st, eframe::APP_KEY))
            .unwrap_or_default();
        Self {
            s,
            applied_on_top: None,
            queue: Vec::new(),
            rx: None,
            cancel: None,
            last_summary: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn add_paths(&mut self, paths: &[PathBuf]) {
        if self.is_running() {
            return;
        }
        let discovered = scan_inputs(paths, self.s.config.recursive);
        for item in discovered {
            let canonical_new =
                std::fs::canonicalize(&item.source_path).unwrap_or_else(|_| item.source_path.clone());
            let already_queued = self.queue.iter().any(|q| {
                let c = std::fs::canonicalize(&q.file.source_path)
                    .unwrap_or_else(|_| q.file.source_path.clone());
                c == canonical_new
            });
            if !already_queued {
                self.queue.push(QueueEntry {
                    file: item,
                    status: ItemStatus::Ready,
                });
            }
        }
    }

    pub fn start_conversion(&mut self, ctx: &egui::Context) {
        if self.is_running() || self.queue.is_empty() {
            return;
        }
        for entry in &mut self.queue {
            entry.status = ItemStatus::Ready;
        }
        self.last_summary = None;

        let items: Vec<DiscoveredFile> = self.queue.iter().map(|q| q.file.clone()).collect();
        let cfg = self.s.config.clone();
        let ctx_clone = ctx.clone();
        let (rx, cancel) = spawn_batch(
            items,
            cfg,
            Some(Box::new(move || {
                ctx_clone.request_repaint();
            })),
        );
        self.rx = Some(rx);
        self.cancel = Some(cancel);
    }

    pub fn cancel_conversion(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn poll_worker(&mut self) {
        let mut finished = false;
        if let Some(rx) = &self.rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    WorkerEvent::FileStarted { index } => {
                        if let Some(item) = self.queue.get_mut(index) {
                            item.status = ItemStatus::Working;
                        }
                    }
                    WorkerEvent::FileCompleted {
                        index,
                        outputs,
                        was_animated,
                        duration_ms,
                    } => {
                        if let Some(item) = self.queue.get_mut(index) {
                            item.status = ItemStatus::Done {
                                outputs,
                                was_animated,
                                duration_ms,
                            };
                        }
                    }
                    WorkerEvent::FileSkipped {
                        index,
                        path: _,
                        reason,
                    } => {
                        if let Some(item) = self.queue.get_mut(index) {
                            item.status = ItemStatus::Skipped { reason };
                        }
                    }
                    WorkerEvent::FileFailed { index, error } => {
                        if let Some(item) = self.queue.get_mut(index) {
                            item.status = ItemStatus::Failed { error };
                        }
                    }
                    WorkerEvent::BatchFinished {
                        converted,
                        skipped,
                        failed,
                        cancelled,
                    } => {
                        self.last_summary = Some(BatchSummary {
                            converted,
                            skipped,
                            failed,
                            cancelled,
                        });
                        finished = true;
                    }
                }
            }
        }
        if finished {
            self.rx = None;
            self.cancel = None;
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.add_paths(&dropped);
        }
    }

    fn title_bar(&mut self, ui: &mut Ui) {
        chrome::title_bar_drag(ui);
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new("WEBP")
                    .font(FontId::new(19.0, FontFamily::Monospace))
                    .color(theme::ACCENT)
                    .strong(),
            );
            ui.label(
                RichText::new("//")
                    .font(FontId::new(19.0, FontFamily::Monospace))
                    .color(theme::LINE_STRONG),
            );
            ui.label(
                RichText::new("CONVERTER")
                    .font(FontId::new(16.0, FontFamily::Monospace))
                    .color(theme::INK),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if chrome::window_button(ui, WindowButton::Close, chrome::TITLE_H - 6.0).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if chrome::window_button(ui, WindowButton::Minimize, chrome::TITLE_H - 6.0).clicked()
                {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ui.add_space(6.0);
                if hud_button(ui, "PIN", theme::ACCENT, self.s.on_top)
                    .on_hover_text("Keep window always on top")
                    .clicked()
                {
                    self.s.on_top = !self.s.on_top;
                }
            });
        });
    }

    fn drop_zone(&mut self, ui: &mut Ui) {
        let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let border_color = if hovering_files {
            theme::ACCENT
        } else {
            theme::LINE_STRONG
        };
        let fill_color = if hovering_files {
            theme::PANEL.lerp_to_gamma(theme::ACCENT, 0.12)
        } else {
            theme::PANEL
        };

        let frame = egui::Frame::new()
            .fill(fill_color)
            .stroke(Stroke::new(1.0, border_color))
            .inner_margin(Margin::symmetric(14, 10));

        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("■ DROP .WEBP FILES OR FOLDERS HERE")
                            .font(FontId::new(15.0, FontFamily::Monospace))
                            .color(if hovering_files {
                                theme::ACCENT
                            } else {
                                theme::INK
                            })
                            .strong(),
                    );
                    caption(
                        ui,
                        "Static & Animated WebP -> GIF, PNG, JPG, BMP, ICO",
                    );
                });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(!self.is_running(), |ui| {
                        if hud_button(ui, "CLEAR", theme::MUTED, false)
                            .on_hover_text("Clear conversion queue")
                            .clicked()
                        {
                            self.queue.clear();
                            self.last_summary = None;
                        }
                        if hud_button(ui, "+ FOLDER", theme::ACCENT, false)
                            .on_hover_text("Select a folder of WebP files")
                            .clicked()
                            && let Some(folder) = rfd::FileDialog::new().pick_folder()
                        {
                            self.add_paths(&[folder]);
                        }
                        if hud_button(ui, "+ FILES", theme::CYAN, false)
                            .on_hover_text("Select one or more .webp files")
                            .clicked()
                            && let Some(files) = rfd::FileDialog::new()
                                .add_filter("WebP Images", &["webp", "WEBP"])
                                .pick_files()
                        {
                            self.add_paths(&files);
                        }
                    });
                });
            });
        });
    }

    fn settings_panel(&mut self, ui: &mut Ui) {
        let frame = egui::Frame::new()
            .fill(theme::PANEL)
            .stroke(Stroke::new(1.0, theme::LINE))
            .inner_margin(Margin::symmetric(14, 10));

        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled_ui(!self.is_running(), |ui| {
                // Row 1: Target Format
                ui.horizontal_wrapped(|ui| {
                    caption(ui, "FORMAT");
                    for fmt in TargetFormat::ALL {
                        let sel = self.s.config.format == fmt;
                        if hud_button(ui, fmt.label(), theme::ACCENT, sel).clicked() {
                            self.s.config.format = fmt;
                        }
                    }
                });

                // Row 2: Animated -> Static policy (when explicit static format is selected)
                if self.s.config.format.is_explicit_static() {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        caption(ui, "ANIMATED -> STATIC");
                        for pol in [AnimatedPolicy::FirstFrame, AnimatedPolicy::AllFrames] {
                            let sel = self.s.config.animated_policy == pol;
                            if hud_button(ui, pol.label(), theme::CYAN, sel).clicked() {
                                self.s.config.animated_policy = pol;
                            }
                        }
                    });
                }

                ui.add_space(4.0);
                // Row 3: Folder & Existing file policies
                ui.horizontal(|ui| {
                    if hud_button(
                        ui,
                        "RECURSIVE",
                        theme::GOOD,
                        self.s.config.recursive,
                    )
                    .on_hover_text("Scan subfolders when adding a directory")
                    .clicked()
                    {
                        self.s.config.recursive = !self.s.config.recursive;
                    }

                    if hud_button(
                        ui,
                        "DEL ORIGINAL",
                        theme::BAD,
                        self.s.config.delete_original,
                    )
                    .on_hover_text("Delete original .webp after verified conversion")
                    .clicked()
                    {
                        self.s.config.delete_original = !self.s.config.delete_original;
                    }

                    ui.separator();
                    caption(ui, "IF EXISTS");
                    for pol in [
                        ExistingPolicy::Skip,
                        ExistingPolicy::Overwrite,
                        ExistingPolicy::Rename,
                    ] {
                        let sel = self.s.config.existing_policy == pol;
                        if hud_button(ui, pol.label(), theme::ACCENT, sel).clicked() {
                            self.s.config.existing_policy = pol;
                        }
                    }
                });

                ui.add_space(4.0);
                // Row 4: Quality / Speed sliders
                ui.horizontal(|ui| {
                    caption(ui, "JPG QUALITY");
                    ui.add(
                        egui::Slider::new(&mut self.s.config.jpg_quality, 1..=100)
                            .suffix("%"),
                    );
                    ui.add_space(8.0);
                    caption(ui, "GIF SPEED");
                    ui.add(egui::Slider::new(&mut self.s.config.gif_speed, 1..=30))
                        .on_hover_text("1 = highest quality / slowest, 30 = fastest");
                });

                ui.add_space(4.0);
                // Row 5: Output directory
                ui.horizontal(|ui| {
                    caption(ui, "OUTPUT DIR");
                    let label_text = match &self.s.config.output_dir {
                        Some(p) => p.display().to_string(),
                        None => "SAME AS SOURCE FOLDER".to_string(),
                    };
                    small(ui, &label_text, theme::INK);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.s.config.output_dir.is_some()
                            && hud_button(ui, "SAME AS SOURCE", theme::MUTED, false).clicked()
                        {
                            self.s.config.output_dir = None;
                        }
                        if hud_button(ui, "BROWSE…", theme::ACCENT, false).clicked()
                            && let Some(dir) = rfd::FileDialog::new().pick_folder()
                        {
                            self.s.config.output_dir = Some(dir);
                        }
                    });
                });
            });
        });
    }

    fn action_and_progress_bar(&mut self, ui: &mut Ui) {
        let total = self.queue.len();
        let mut done_count = 0usize;
        let mut skip_count = 0usize;
        let mut fail_count = 0usize;

        for q in &self.queue {
            match q.status {
                ItemStatus::Done { .. } => done_count += 1,
                ItemStatus::Skipped { .. } => skip_count += 1,
                ItemStatus::Failed { .. } => fail_count += 1,
                ItemStatus::Ready | ItemStatus::Working => {}
            }
        }
        let finished_count = done_count + skip_count + fail_count;
        let progress = if total == 0 {
            0.0
        } else {
            finished_count as f32 / total as f32
        };

        ui.horizontal(|ui| {
            if self.is_running() {
                if hud_button(ui, "■ CANCEL BATCH", theme::BAD, true).clicked() {
                    self.cancel_conversion();
                }
            } else {
                let label = format!("▶ CONVERT ({total})");
                let can_run = total > 0;
                ui.add_enabled_ui(can_run, |ui| {
                    if hud_button(ui, &label, theme::ACCENT, can_run).clicked() {
                        let ctx = ui.ctx().clone();
                        self.start_conversion(&ctx);
                    }
                });
            }

            ui.add_space(8.0);
            let bar_w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(bar_w, 28.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::ZERO, theme::FIELD);
            p.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, theme::LINE_STRONG),
                StrokeKind::Inside,
            );
            if progress > 0.0 {
                let fill_w = (rect.width() * progress).clamp(0.0, rect.width());
                let fill_rect = Rect::from_min_size(rect.min, vec2(fill_w, rect.height()));
                p.rect_filled(
                    fill_rect,
                    CornerRadius::ZERO,
                    theme::ACCENT.gamma_multiply(0.35),
                );
            }
            let status_str = if total == 0 {
                "QUEUE EMPTY".to_string()
            } else {
                format!(
                    "{finished_count}/{total}  |  OK: {done_count}  SKIP: {skip_count}  ERR: {fail_count}"
                )
            };
            p.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                status_str,
                FontId::new(13.0, FontFamily::Monospace),
                theme::INK,
            );
        });
    }

    fn queue_list(&mut self, ui: &mut Ui) {
        let frame = egui::Frame::new()
            .fill(theme::FIELD)
            .stroke(Stroke::new(1.0, theme::LINE))
            .inner_margin(Margin::symmetric(10, 8));

        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());

            if self.queue.is_empty() {
                ui.centered_and_justified(|ui| {
                    caption(
                        ui,
                        "NO .WEBP FILES QUEUED — DROP FILES/FOLDERS ABOVE OR CLICK + FILES / + FOLDER",
                    );
                });
                return;
            }

            let row_h = 26.0;
            let total_rows = self.queue.len();
            let running = self.is_running();
            let mut remove_idx: Option<usize> = None;

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_h, total_rows, |ui, row_range| {
                    for idx in row_range {
                        let Some(entry) = self.queue.get(idx) else {
                            continue;
                        };
                        ui.horizontal(|ui| {
                            let (badge, color, tip) = match &entry.status {
                                ItemStatus::Ready => ("READY", theme::MUTED, String::new()),
                                ItemStatus::Working => ("WORK", theme::ACCENT, String::new()),
                                ItemStatus::Done {
                                    outputs,
                                    was_animated,
                                    duration_ms,
                                } => {
                                    let kind = if *was_animated { "ANIM" } else { "STATIC" };
                                    let out_str = outputs
                                        .first()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default();
                                    (
                                        "DONE",
                                        theme::GOOD,
                                        format!("{kind} -> {out_str} ({duration_ms} ms)"),
                                    )
                                }
                                ItemStatus::Skipped { reason } => {
                                    ("SKIP", theme::CYAN, reason.clone())
                                }
                                ItemStatus::Failed { error } => {
                                    ("ERR", theme::BAD, error.clone())
                                }
                            };

                            let badge_resp = ui.label(
                                RichText::new(format!("[{badge:^5}]"))
                                    .font(FontId::new(13.0, FontFamily::Monospace))
                                    .color(color)
                                    .strong(),
                            );
                            if !tip.is_empty() {
                                badge_resp.on_hover_text(&tip);
                            }

                            let file_name = entry
                                .file
                                .source_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown.webp");
                            let full_display = entry.file.source_path.display().to_string();
                            let name_resp = ui.label(
                                RichText::new(file_name)
                                    .font(FontId::new(14.0, FontFamily::Monospace))
                                    .color(theme::INK),
                            );
                            name_resp.on_hover_text(full_display);

                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if !running
                                    && ui
                                        .small_button(
                                            RichText::new("×")
                                                .font(FontId::new(14.0, FontFamily::Monospace))
                                                .color(theme::MUTED),
                                        )
                                        .on_hover_text("Remove from queue")
                                        .clicked()
                                {
                                    remove_idx = Some(idx);
                                }
                            });
                        });
                    }
                });

            if let Some(i) = remove_idx {
                self.queue.remove(i);
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker();
        self.handle_dropped_files(ctx);

        if self.applied_on_top != Some(self.s.on_top) {
            let level = if self.s.on_top {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.applied_on_top = Some(self.s.on_top);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(WINDOW_W, WINDOW_H)));
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let inset = chrome::FRAME_INSET as i8;

        egui::Panel::top("title-bar")
            .frame(
                egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                    left: inset + 10,
                    right: inset - 2,
                    top: 4,
                    bottom: 0,
                }),
            )
            .exact_size(chrome::TITLE_H)
            .resizable(false)
            .show_separator_line(false)
            .show(ui, |ui| self.title_bar(ui));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                    left: inset + 10,
                    right: inset + 10,
                    top: 6,
                    bottom: inset + 8,
                }),
            )
            .show(ui, |ui| {
                self.drop_zone(ui);
                ui.add_space(8.0);
                self.settings_panel(ui);
                ui.add_space(8.0);
                self.action_and_progress_bar(ui);
                ui.add_space(8.0);
                self.queue_list(ui);
            });

        chrome::window_frame(&ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.s);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG.to_normalized_gamma_f32()
    }
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(FontId::new(13.0, FontFamily::Monospace))
            .color(theme::MUTED),
    );
}

fn small(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(
        RichText::new(text)
            .font(FontId::new(13.0, FontFamily::Monospace))
            .color(color),
    );
}

fn hud_button(ui: &mut Ui, text: &str, color: Color32, selected: bool) -> Response {
    let font = FontId::new(13.0, FontFamily::Monospace);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), color);
    let size = galley.size() + vec2(16.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hovered = resp.hovered();
    let (fill, ink, stroke) = if selected {
        (
            color.gamma_multiply(0.22),
            color,
            Stroke::new(1.0, color),
        )
    } else if hovered {
        (
            theme::LINE,
            theme::INK,
            Stroke::new(1.0, color.gamma_multiply(0.7)),
        )
    } else {
        (
            theme::FIELD,
            theme::MUTED,
            Stroke::new(1.0, theme::LINE_STRONG),
        )
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, fill);
    p.rect_stroke(rect, CornerRadius::ZERO, stroke, StrokeKind::Inside);
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font,
        ink,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_renders_and_queues_files() {
        let tmp = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let webp_path = tmp.path().join("test.webp");
        img.save(&webp_path).unwrap();

        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(WINDOW_W, WINDOW_H))
            .build_eframe(|cc| App::new(cc));
        {
            let app = harness.state_mut();
            app.add_paths(&[webp_path.clone(), webp_path]);
            assert_eq!(app.queue_len(), 1);
        }
        harness.run_steps(2);
    }
}
