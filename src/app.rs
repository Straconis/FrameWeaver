//! The FrameWeaver window.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, TextStyle, Vec2};

use crate::cleanup::{self, Cleanup};
use crate::ffmpeg::{self, Encoding, Ffmpeg, State};
use crate::input::{self, Pictures};
use crate::settings::Settings;
use crate::timing::{self, Mode, Plan};
use crate::video::{self, Aspect, Fit, Format, Job, Quality, Size};

const BRAND: Color32 = Color32::from_rgb(0x3B, 0x82, 0xF6);
const BRAND_CYAN: Color32 = Color32::from_rgb(0x67, 0xE8, 0xF9);
const STOP_RED: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const WARN: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);

/// Width of the label column, so every section's settings line up.
const LABEL_WIDTH: f32 = 160.0;

const ICON_PNG: &[u8] = include_bytes!("../branding/out/icons/frameweaver-256.png");
const LOGO_PNG: &[u8] = include_bytes!("../branding/out/icons/frameweaver-128.png");
const MAKER_PNG: &[u8] = include_bytes!("../branding/out/straconis-software-wordmark.png");
/// Height over width of the wordmark picture (256×84).
const MAKER_ASPECT: f32 = 84.0 / 256.0;
const MAKER_URL: &str = "https://straconis.dev";

pub fn run() -> Result<(), String> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FrameWeaver")
        .with_app_id("frameweaver")
        .with_inner_size([560.0, 860.0])
        .with_min_inner_size([460.0, 420.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(ICON_PNG) {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "FrameWeaver",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
    .map_err(|e| e.to_string())
}

struct App {
    settings: Settings,
    saved_settings: Settings,
    ffmpeg: Result<Ffmpeg, String>,
    pictures: Option<Arc<Pictures>>,
    /// The picked path, while its pictures are being read in the background.
    loading: Option<(PathBuf, Receiver<Result<Pictures, String>>)>,
    load_error: Option<String>,
    file_name: String,
    encoding: Option<Encoding>,
    logo: Option<egui::TextureHandle>,
    maker: Option<egui::TextureHandle>,
    /// Old videos waiting for the user to confirm they should be deleted.
    cleanup: Option<Cleanup>,
    /// What the last cleanup did, and whether it went wrong.
    cleanup_note: Option<(String, bool)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_style(&cc.egui_ctx);
        let settings = Settings::load();
        App {
            ffmpeg: Ffmpeg::find(settings.ffmpeg_path.as_deref()),
            saved_settings: settings.clone(),
            settings,
            pictures: None,
            loading: None,
            load_error: None,
            file_name: String::new(),
            encoding: None,
            logo: load_logo(&cc.egui_ctx),
            maker: load_texture(&cc.egui_ctx, "maker", MAKER_PNG),
            cleanup: None,
            cleanup_note: None,
        }
    }

    fn save_settings_if_changed(&mut self) {
        if self.settings != self.saved_settings {
            self.settings.save();
            self.saved_settings = self.settings.clone();
        }
    }

    fn is_encoding(&self) -> bool {
        self.encoding.as_ref().is_some_and(|e| e.is_running())
    }

    /// Reads the chosen pictures on a background thread, so a big ZIP never freezes the window.
    fn open(&mut self, path: PathBuf, ctx: &egui::Context) {
        self.settings.last_input_folder = path.parent().map(Path::to_path_buf);
        let (send, receive) = channel();
        let (worker_path, ctx) = (path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = send.send(input::load(&worker_path));
            ctx.request_repaint();
        });
        self.loading = Some((path, receive));
        self.load_error = None;
    }

    fn check_loading(&mut self) {
        let Some((_, receive)) = &self.loading else {
            return;
        };
        if let Ok(result) = receive.try_recv() {
            self.loading = None;
            match result {
                Ok(pictures) => {
                    self.file_name = pictures.name.clone();
                    self.pictures = Some(Arc::new(pictures));
                }
                Err(e) => self.load_error = Some(e),
            }
        }
    }

    fn plan(&self) -> Result<Plan, String> {
        let count = self.pictures.as_ref().map_or(0, |p| p.files.len());
        if count == 0 {
            return Err("Choose some pictures first.".into());
        }
        match self.settings.mode {
            Mode::TotalLength => {
                let length = self.settings.length();
                if length.is_zero() {
                    return Err("Set a video length longer than zero.".into());
                }
                Ok(timing::plan_total(count, length, self.settings.fps))
            }
            Mode::PerPicture => Ok(timing::plan_per_picture(
                count,
                Duration::from_secs_f64(self.settings.per_picture.clamp(0.01, 3600.0)),
                self.settings.fps,
            )),
        }
    }

    fn output_size(&self) -> (u32, u32) {
        let source = self
            .pictures
            .as_ref()
            .and_then(|p| p.first_size)
            .unwrap_or((1920, 1080));
        video::output_size(
            source,
            self.settings.size,
            self.settings.custom_size,
            self.settings.aspect,
        )
    }

    fn output_path(&self) -> PathBuf {
        let name = clean_file_name(&self.file_name);
        ffmpeg::free_name(
            &self.settings.output_folder,
            &name,
            self.settings.format.extension(),
        )
    }

    fn start(&mut self) {
        let (Ok(ffmpeg), Some(pictures), Ok(plan)) = (&self.ffmpeg, &self.pictures, self.plan())
        else {
            return;
        };
        let job = Job {
            plan,
            format: self.settings.format,
            quality: self.settings.quality,
            size: self.output_size(),
            fit: self.settings.fit,
            bar_color: self.settings.bar_color,
        };
        self.encoding = Some(Encoding::start(
            ffmpeg,
            pictures.clone(),
            job,
            self.output_path(),
        ));
    }

    // ---- sections ----------------------------------------------------------------------------

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new((logo.id(), Vec2::splat(40.0))));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(RichText::new("Frame").size(24.0).strong());
                    ui.label(RichText::new("Weaver").size(24.0));
                });
                ui.label(RichText::new("Turn pictures into a time-lapse video").weak());
            });
        });
    }

    fn pictures_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Pictures");
        ui.horizontal(|ui| {
            if ui
                .button("Choose folder…")
                .on_hover_text("A folder of pictures, such as a SnapLapse recording")
                .clicked()
            {
                let picker = self.picker();
                if let Some(folder) = crate::dialog::run(|| picker.pick_folder()) {
                    self.open(folder, ui.ctx());
                }
            }
            if ui
                .button("Choose file or ZIP…")
                .on_hover_text("A picture, or a ZIP of pictures (for split ZIPs, any part)")
                .clicked()
            {
                let picker = self
                    .picker()
                    .add_filter("Pictures or ZIP", &["zip", "png", "jpg", "jpeg", "webp"]);
                if let Some(file) = crate::dialog::run(|| picker.pick_file()) {
                    self.open(file, ui.ctx());
                }
            }
        });
        ui.add_space(4.0);
        if let Some((path, _)) = &self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Reading {}…",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
            });
        } else if let Some(error) = &self.load_error {
            ui.label(RichText::new(error).color(WARN));
        } else if let Some(pictures) = &self.pictures {
            let size = pictures
                .first_size
                .map_or(String::new(), |(w, h)| format!(" · {w}×{h}"));
            ui.label(format!(
                "{} pictures from \"{}\"{size}",
                pictures.files.len(),
                pictures.name
            ));
            if !pictures.missing_parts.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(pictures.missing_parts_warning()).color(WARN))
                        .wrap(),
                );
            }
        } else {
            ui.label(RichText::new("No pictures chosen yet.").weak());
        }
    }

    fn picker(&self) -> rfd::FileDialog {
        let dialog = rfd::FileDialog::new();
        match &self.settings.last_input_folder {
            Some(folder) if folder.is_dir() => dialog.set_directory(folder),
            _ => dialog,
        }
    }

    fn length_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Length");
        egui::Grid::new("length")
            .num_columns(2)
            .min_col_width(LABEL_WIDTH)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Set by");
                egui::ComboBox::from_id_salt("mode")
                    .selected_text(self.settings.mode.label())
                    .show_ui(ui, |ui| {
                        for m in [Mode::TotalLength, Mode::PerPicture] {
                            ui.selectable_value(&mut self.settings.mode, m, m.label());
                        }
                    });
                ui.end_row();

                match self.settings.mode {
                    Mode::TotalLength => {
                        ui.label("Video length");
                        ui.horizontal(|ui| {
                            // One box each for hours, minutes and seconds. Type a number or drag.
                            let boxes = [
                                (&mut self.settings.length_hours, 99, "hours"),
                                (&mut self.settings.length_minutes, 59, "minutes"),
                                (&mut self.settings.length_seconds, 59, "seconds"),
                            ];
                            for (value, max, unit) in boxes {
                                ui.add(
                                    egui::DragValue::new(value)
                                        .range(0..=max)
                                        .speed(0.1)
                                        .min_decimals(0),
                                );
                                ui.label(RichText::new(unit).weak());
                                ui.add_space(4.0);
                            }
                        });
                    }
                    Mode::PerPicture => {
                        ui.label("Each picture");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.settings.per_picture)
                                    .range(0.01..=3600.0)
                                    .speed(0.05)
                                    .max_decimals(2),
                            );
                            ui.label(RichText::new("seconds").weak());
                        });
                    }
                }
                ui.end_row();

                ui.label("Frame rate");
                egui::ComboBox::from_id_salt("fps")
                    .selected_text(format!("{} fps", self.settings.fps))
                    .show_ui(ui, |ui| {
                        for fps in video::FRAME_RATES {
                            ui.selectable_value(&mut self.settings.fps, fps, format!("{fps} fps"));
                        }
                    });
                ui.end_row();
            });
    }

    fn size_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Size and shape");
        egui::Grid::new("size")
            .num_columns(2)
            .min_col_width(LABEL_WIDTH)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Size");
                ui.horizontal(|ui| {
                    combo(ui, "size", &mut self.settings.size, &Size::ALL, Size::label);
                    if self.settings.size == Size::Custom {
                        let (w, h) = &mut self.settings.custom_size;
                        ui.add(egui::DragValue::new(w).range(16..=8192).suffix(" w"));
                        ui.label("×");
                        ui.add(egui::DragValue::new(h).range(16..=8192).suffix(" h"));
                    }
                });
                ui.end_row();

                if self.settings.size != Size::Custom {
                    ui.label("Shape");
                    combo(
                        ui,
                        "aspect",
                        &mut self.settings.aspect,
                        &Aspect::ALL,
                        Aspect::label,
                    );
                    ui.end_row();
                }

                ui.label("Pictures that don't fit");
                ui.horizontal(|ui| {
                    combo(ui, "fit", &mut self.settings.fit, &Fit::ALL, Fit::label);
                    if self.settings.fit == Fit::Bars {
                        ui.label("Bars");
                        egui::color_picker::color_edit_button_srgb(
                            ui,
                            &mut self.settings.bar_color,
                        )
                        .on_hover_text("Bar colour");
                    }
                });
                ui.end_row();
            });
    }

    fn output_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "Save as");
        egui::Grid::new("output")
            .num_columns(2)
            .min_col_width(LABEL_WIDTH)
            .spacing([14.0, 10.0])
            .show(ui, |ui| {
                ui.label("Folder");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button("Clean up…")
                        .on_hover_text("Delete the videos in this folder. Asks first.")
                        .clicked()
                    {
                        let found = cleanup::scan(&self.settings.output_folder);
                        if found.is_empty() {
                            self.cleanup_note = Some((
                                "There are no videos in this folder to delete.".into(),
                                false,
                            ));
                        } else {
                            self.cleanup = Some(found);
                        }
                    }
                    if ui
                        .button("Open")
                        .on_hover_text("Show this folder")
                        .clicked()
                    {
                        let _ = std::fs::create_dir_all(&self.settings.output_folder);
                        open_path(&self.settings.output_folder);
                    }
                    if ui.button("Browse…").clicked() {
                        let picker =
                            rfd::FileDialog::new().set_directory(&self.settings.output_folder);
                        if let Some(folder) = crate::dialog::run(|| picker.pick_folder()) {
                            self.settings.output_folder = folder;
                        }
                    }
                    let full = self.settings.output_folder.display().to_string();
                    ui.add(egui::Label::new(&full).truncate())
                        .on_hover_text(&full);
                });
                ui.end_row();

                if let Some((note, is_error)) = &self.cleanup_note {
                    ui.label("");
                    let text = RichText::new(note);
                    ui.add(
                        egui::Label::new(if *is_error {
                            text.color(WARN)
                        } else {
                            text.weak()
                        })
                        .wrap(),
                    );
                    ui.end_row();
                }

                ui.label("File name");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.file_name)
                            .desired_width(260.0)
                            .hint_text("video"),
                    );
                    ui.label(
                        RichText::new(format!(".{}", self.settings.format.extension())).weak(),
                    );
                });
                ui.end_row();

                ui.label("Format");
                let supported: Vec<Format> = Format::ALL
                    .into_iter()
                    .filter(|f| self.ffmpeg.as_ref().map_or(true, |ff| ff.supports(*f)))
                    .collect();
                combo(
                    ui,
                    "format",
                    &mut self.settings.format,
                    &supported,
                    Format::label,
                );
                ui.end_row();

                ui.label("Quality");
                combo(
                    ui,
                    "quality",
                    &mut self.settings.quality,
                    &Quality::ALL,
                    Quality::label,
                );
                ui.end_row();

                ui.label("When done");
                ui.horizontal(|ui| {
                    toggle(ui, &mut self.settings.open_when_done);
                    let label =
                        ui.add(egui::Label::new("Open the video").sense(egui::Sense::click()));
                    if label.clicked() {
                        self.settings.open_when_done = !self.settings.open_when_done;
                    }
                });
                ui.end_row();
            });
    }

    fn summary(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match self.plan() {
                Ok(plan) => {
                    let (w, h) = self.output_size();
                    ui.label(plan.summary());
                    let detail = format!("Video: {w}×{h}, {}.", self.settings.format.label());
                    ui.label(RichText::new(detail).weak());
                    if plan.skipped() > 0 {
                        ui.label(
                            RichText::new(
                                "Tip: a longer length or a higher frame rate shows every picture.",
                            )
                            .color(WARN),
                        );
                    }
                    if self.settings.format == Format::Gif
                        && plan.length() > Duration::from_secs(60)
                    {
                        ui.label(
                            RichText::new(
                                "GIFs get very large past a minute or so. MP4 is much smaller.",
                            )
                            .color(WARN),
                        );
                    }
                }
                Err(message) => {
                    ui.label(RichText::new(message).weak());
                }
            }
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        if let Some(encoding) = &self.encoding {
            let progress = &encoding.progress;
            match progress.state() {
                State::Running => {
                    let text = match progress.time_left() {
                        Some(left) => format!(
                            "{:.0}% · about {} left",
                            progress.fraction() * 100.0,
                            timing::format_hms(left)
                        ),
                        None => format!("{:.0}%", progress.fraction() * 100.0),
                    };
                    ui.add(
                        egui::ProgressBar::new(progress.fraction())
                            .text(text)
                            .fill(BRAND),
                    );
                    ui.add_space(8.0);
                    if big_button(ui, "Cancel", STOP_RED, true).clicked() {
                        encoding.cancel();
                    }
                    return;
                }
                State::Done(path) => {
                    ui.label(RichText::new("Video saved").strong().color(BRAND_CYAN));
                    ui.add(egui::Label::new(path.display().to_string()).truncate());
                    ui.horizontal(|ui| {
                        if ui.button("Play video").clicked() {
                            open_path(&path);
                        }
                        if ui.button("Show folder").clicked() {
                            open_path(path.parent().unwrap_or(Path::new(".")));
                        }
                    });
                    ui.add_space(8.0);
                }
                State::Cancelled => {
                    ui.label(RichText::new("Cancelled. No file was saved.").weak());
                    ui.add_space(8.0);
                }
                State::Failed(message) => {
                    ui.add(egui::Label::new(RichText::new(message).color(STOP_RED)).wrap());
                    ui.add_space(8.0);
                }
            }
        }
        let ready = self.ffmpeg.is_ok()
            && self.pictures.is_some()
            && self.loading.is_none()
            && self.plan().is_ok();
        if big_button(ui, "Create video", BRAND, ready).clicked() {
            self.start();
        }
        if let Err(problem) = &self.ffmpeg {
            ui.add(egui::Label::new(RichText::new(problem).color(WARN)).wrap());
        }
    }

    /// Asks before deleting anything, listing exactly what will go.
    fn confirm_cleanup(&mut self, ctx: &egui::Context) {
        let Some(found) = &self.cleanup else {
            return;
        };
        let count = found.items.len();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("confirm_cleanup")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.label(RichText::new("Delete old videos?").size(17.0).strong());
            ui.add_space(6.0);
            ui.label(format!(
                "This permanently deletes {count} {} ({}) from {}. They don't go to the Recycle \
                 Bin or Trash. Only MP4, WebM and GIF files are deleted; other files and \
                 folders are left alone.",
                plural(count),
                cleanup::human_size(found.bytes),
                self.settings.output_folder.display(),
            ));
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    for item in &found.items {
                        let name = item.file_name().unwrap_or_default().to_string_lossy();
                        ui.add(egui::Label::new(RichText::new(name).weak()).truncate());
                    }
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let delete =
                    egui::Button::new(RichText::new("Delete").strong().color(Color32::WHITE))
                        .fill(STOP_RED);
                if ui.add(delete).clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(false);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                let found = self.cleanup.take().unwrap_or_default();
                let (removed, error) = cleanup::delete(&found);
                // The "Video saved" message may point at a video that's just been deleted.
                self.encoding = None;
                self.cleanup_note = Some(match error {
                    None => (format!("Deleted {removed} {}.", plural(removed)), false),
                    Some(e) => (format!("Deleted {removed} of {count}, but {e}"), true),
                });
            }
            Some(false) => self.cleanup = None,
            None => {}
        }
    }

    fn footer(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Ok(ffmpeg) = &self.ffmpeg {
                let version = ffmpeg
                    .version
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                ui.label(RichText::new(format!("Using {version}")).small().weak())
                    .on_hover_text(ffmpeg.path.display().to_string());
            }
            if let Some(maker) = &self.maker {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    maker_mark(ui, maker)
                });
            }
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_loading();
        self.save_settings_if_changed();
        let busy = self.is_encoding();
        if busy || self.loading.is_some() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        // Open the finished video once, if asked.
        if let Some(encoding) = &self.encoding {
            if let (false, State::Done(path)) = (busy, encoding.progress.state()) {
                if self.settings.open_when_done && !encoding_opened(encoding) {
                    open_path(&path);
                }
            }
        }
        // The summary, Create button and progress stay pinned to the bottom, always visible.
        let bottom =
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(20, 14));
        egui::TopBottomPanel::bottom("actions")
            .frame(bottom)
            .show(ctx, |ui| {
                self.summary(ui);
                ui.add_space(10.0);
                self.controls(ui);
                ui.add_space(6.0);
                self.footer(ui);
            });
        let panel = egui::Frame::central_panel(&ctx.style()).inner_margin(20.0);
        egui::CentralPanel::default().frame(panel).show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    self.header(ui);
                    ui.add_space(12.0);
                    ui.add_enabled_ui(!busy, |ui| {
                        self.pictures_section(ui);
                        self.length_section(ui);
                        self.size_section(ui);
                        self.output_section(ui);
                    });
                });
        });
        if !busy {
            self.confirm_cleanup(ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Dropping the encoding cancels it and removes any half-made file.
        self.encoding = None;
        self.settings.save();
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        "video"
    } else {
        "videos"
    }
}

/// Remembers, per finished video, that it has already been opened.
fn encoding_opened(encoding: &Encoding) -> bool {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static LAST: AtomicUsize = AtomicUsize::new(0);
    let id = Arc::as_ptr(&encoding.progress) as usize;
    LAST.swap(id, Ordering::Relaxed) == id
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong().size(16.0));
    ui.add_space(4.0);
}

fn combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: fn(T) -> &'static str,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for &option in options {
                ui.selectable_value(value, option, label(option));
            }
        });
}

fn big_button(ui: &mut egui::Ui, text: &str, fill: Color32, enabled: bool) -> egui::Response {
    let button = egui::Button::new(
        RichText::new(text)
            .size(17.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(fill)
    .min_size(Vec2::new(ui.available_width(), 44.0));
    ui.add_enabled(enabled, button)
}

/// Removes characters file names can't contain.
fn clean_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() {
        "video".into()
    } else {
        cleaned.to_string()
    }
}

/// Opens a file with its usual app, a folder in the file manager, or a web address in the
/// browser.
fn open_path(path: &Path) {
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(not(windows))]
    let opener = "xdg-open";
    launch(std::process::Command::new(opener).arg(path));
}

/// An on/off switch, which reads more clearly than a checkbox. Adapted from egui's example.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(40.0, 22.0), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });
    if ui.is_rect_visible(rect) {
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
        let visuals = ui.style().interact_selectable(&response, *on);
        let fill = if *on { BRAND } else { visuals.bg_fill };
        let radius = 0.5 * rect.height();
        ui.painter().rect(
            rect,
            radius,
            fill,
            visuals.bg_stroke,
            egui::StrokeKind::Inside,
        );
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), how_on);
        ui.painter().circle(
            egui::pos2(x, rect.center().y),
            0.75 * radius,
            Color32::WHITE,
            egui::Stroke::NONE,
        );
    }
    response
}

fn apply_style(ctx: &egui::Context) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            style.visuals.selection.bg_fill = BRAND;
            style.visuals.hyperlink_color = BRAND;
            style.spacing.button_padding = Vec2::new(10.0, 5.0);
            style.spacing.interact_size.y = 28.0;
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(14.5));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(14.5));
        });
    }
}

fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    load_texture(ctx, "logo", LOGO_PNG)
}

fn load_texture(ctx: &egui::Context, name: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory(png).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(name, pixels, egui::TextureOptions::LINEAR))
}
/// The maker's mark: the Straconis Software wordmark, kept small, linking to the website.
fn maker_mark(ui: &mut egui::Ui, texture: &egui::TextureHandle) {
    let width = 120.0;
    let size = Vec2::new(width, width * MAKER_ASPECT);
    let response = ui
        .add(egui::Image::new((texture.id(), size)).sense(egui::Sense::click()))
        .on_hover_text(format!("Made by Straconis Software. Visit {MAKER_URL}"))
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        open_path(Path::new(MAKER_URL));
    }
}

/// Starts a helper program such as the file manager, and collects it when it exits so it
/// doesn't linger as a finished process while FrameWeaver runs.
fn launch(command: &mut std::process::Command) {
    if let Ok(mut child) = command.spawn() {
        std::thread::spawn(move || child.wait());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_cleaned() {
        assert_eq!(
            clean_file_name("2026-10-14 19-30-05 - VTT: Night"),
            "2026-10-14 19-30-05 - VTT Night"
        );
        assert_eq!(clean_file_name("  ..  "), "video");
    }
}
