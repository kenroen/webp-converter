# WebP Converter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure-Rust CLI and `egui`/`eframe` desktop GUI application that converts static and animated `.webp` files—individually or across entire folders—to `.gif` or static image formats (`.png`, `.jpg`, `.bmp`, `.ico`).

**Architecture:** A single Rust binary (`webp-converter`) sharing a pure-Rust core library (`lib.rs`, `config.rs`, `scanner.rs`, `converter.rs`, `worker.rs`). When invoked with CLI arguments (`std::env::args_os().len() > 1`), it runs a `clap`-driven batch CLI; when launched with no arguments, it opens a borderless `eframe`/`egui` desktop GUI styled consistently with `wardogs-range-calc`.

**Tech Stack:** Rust 2024 (`1.90`), `image` (`0.25`, features: `webp`, `gif`, `png`, `jpeg`, `bmp`, `ico`), `walkdir`, `rayon`, `clap` (`derive`), `eframe`/`egui` (`0.36`), `rfd`, `serde` (`derive`), `tempfile` (dev), `egui_kittest` (dev).

**Spec:** `docs/superpowers/specs/2026-11-07-webp-converter-design.md`

## Global Constraints

- `edition = "2024"` and `rust-version = "1.90"` in `Cargo.toml`
- `[lints.rust] unsafe_code = "forbid"` — 100% safe Rust across all crates/modules
- Release profile optimizations matching `wardogs-range-calc`: `opt-level = "s"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`, and `[profile.dev.package."*"] opt-level = 2`
- Atomic file writes via temporary `.webp-tmp` files before renaming to target path
- `delete_original` must only remove the source `.webp` after all output files for that source are flushed and renamed without error

## Review Focus

1. **Case-insensitive `.webp` extensions (`.WEBP`, `.WebP`) and non-WebP files in scanned directories:** Scanner must match `.webp` case-insensitively and ignore other files (`.png`, `.txt`, `.webp-tmp`).
2. **Duplicate inputs (e.g., passing both a folder and a file inside that folder):** Scanner must deduplicate discovered files while preserving discovery order so two worker threads never race on the same source file.
3. **Transparent RGBA `.webp` converted to `.jpg`:** JPEG does not support `Rgba8`; converter must composite RGBA over an opaque RGB buffer before JPEG encoding instead of failing with `UnsupportedColor`.
4. **Oversized `.webp` (`> 256x256`) converted to `.ico`:** ICO encoder rejects dimensions over `256x256`; converter must downscale proportionally with `FilterType::Lanczos3` so dimensions fit within `256x256`.
5. **Corrupt `.webp` with `delete_original = true`:** Failing to decode or encode must leave the original `.webp` untouched, clean up any `.webp-tmp` file, and report `ConvertOutcome::Failed` without aborting sibling files in a batch.

---

### Task 1: Project Scaffolding & Configuration Model (`Cargo.toml`, `src/config.rs`, `src/lib.rs`)

**Files:**
- Create: `.gitignore`
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/config.rs`

**Interfaces:**
- Produces in `webp_converter::config`:
  - `pub enum TargetFormat { AutoPng, AutoJpg, Gif, Png, Jpg, Bmp, Ico }`
  - `impl TargetFormat`:
    - `pub fn resolve_extension(self, is_animated: bool) -> &'static str`
    - `pub fn is_gif_for(self, is_animated: bool) -> bool`
    - `pub fn label(self) -> &'static str`
  - `pub enum AnimatedPolicy { FirstFrame, AllFrames }`
  - `pub enum ExistingPolicy { Skip, Overwrite, Rename }`
  - `pub struct ConversionConfig { pub format: TargetFormat, pub animated_policy: AnimatedPolicy, pub existing_policy: ExistingPolicy, pub recursive: bool, pub output_dir: Option<PathBuf>, pub delete_original: bool, pub jpg_quality: u8, pub gif_speed: i32 }`

- [ ] **Step 1: Create `.gitignore`, `Cargo.toml`, `src/lib.rs`, and `src/config.rs` with unit tests**

Create `.gitignore`:
```gitignore
/target
```

Create `Cargo.toml`:
```toml
[package]
name = "webp-converter"
description = "Convert static and animated WebP files and folders to GIF, PNG, JPG, BMP, or ICO"
version = "0.1.0"
edition = "2024"
rust-version = "1.90"
publish = false

[lints.rust]
unsafe_code = "forbid"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
eframe = { version = "0.36", default-features = false, features = ["default_fonts", "glow", "persistence"] }
egui = { version = "0.36", default-features = false }
image = { version = "0.25", default-features = false, features = ["webp", "gif", "png", "jpeg", "bmp", "ico"] }
rayon = "1.10"
rfd = "0.15"
serde = { version = "1", features = ["derive"] }
walkdir = "2.5"

[profile.dev.package."*"]
opt-level = 2

[profile.release]
opt-level = "s"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"

[dev-dependencies]
egui_kittest = { version = "0.36", features = ["wgpu", "snapshot", "eframe"] }
tempfile = "3.14"
```

Write tests in `src/config.rs` asserting:
```rust
#[test]
fn target_format_resolves_auto_and_explicit_extensions() {
    assert_eq!(TargetFormat::AutoPng.resolve_extension(false), "png");
    assert_eq!(TargetFormat::AutoPng.resolve_extension(true), "gif");
    assert_eq!(TargetFormat::AutoJpg.resolve_extension(false), "jpg");
    assert_eq!(TargetFormat::AutoJpg.resolve_extension(true), "gif");
    assert_eq!(TargetFormat::Png.resolve_extension(true), "png");
    assert_eq!(TargetFormat::Gif.resolve_extension(false), "gif");
}

#[test]
fn default_config_has_sane_defaults() {
    let cfg = ConversionConfig::default();
    assert_eq!(cfg.format, TargetFormat::AutoPng);
    assert_eq!(cfg.animated_policy, AnimatedPolicy::FirstFrame);
    assert_eq!(cfg.existing_policy, ExistingPolicy::Skip);
    assert!(cfg.recursive);
    assert_eq!(cfg.jpg_quality, 90);
    assert_eq!(cfg.gif_speed, 10);
    assert!(!cfg.delete_original);
}
```

- [ ] **Step 2: Run `cargo test` to verify `config` tests pass**

Run: `cargo test --lib config`
Expected: PASS (2 tests)

- [ ] **Step 3: Commit Task 1**

```powershell
git add .gitignore Cargo.toml Cargo.lock src/lib.rs src/config.rs
git commit -m "feat: initialize crate and conversion configuration types"
```

---

### Task 2: Folder Discovery & Output Path Resolution (`src/scanner.rs`)

**Files:**
- Create: `src/scanner.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::config::{ConversionConfig, ExistingPolicy}`
- Produces in `webp_converter::scanner`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct DiscoveredFile { pub source_path: PathBuf, pub base_dir: Option<PathBuf> }`
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum ResolvedTarget { Ready(PathBuf), SkipExisting(PathBuf) }`
  - `pub fn scan_inputs(inputs: &[PathBuf], recursive: bool) -> Vec<DiscoveredFile>`
  - `pub fn resolve_output_dir(item: &DiscoveredFile, custom_output_dir: Option<&Path>) -> PathBuf`
  - `pub fn resolve_target_path(target_dir: &Path, stem: &str, ext: &str, policy: ExistingPolicy) -> ResolvedTarget`

- [ ] **Step 1: Write failing unit tests in `src/scanner.rs`**

Include tests covering:
- `scan_inputs_recursive_vs_non_recursive`: nested folders `root/a.webp`, `root/sub/b.WEBP`, `root/sub/ignore.png`; verify `recursive = false` returns only `a.webp` and `recursive = true` returns both `a.webp` and `b.WEBP`.
- `scan_inputs_deduplicates_same_file`: passing `[root, root/a.webp]` returns `a.webp` once with its first `base_dir`.
- `resolve_output_dir_preserves_relative_structure`: `item` at `root/sub/deep/b.webp` with `base_dir = Some(root)` and `custom_output_dir = Some(out)` resolves to `out/sub/deep`.
- `resolve_target_path_policies`: when `out/pic.png` already exists:
  - `ExistingPolicy::Skip` -> `ResolvedTarget::SkipExisting(".../pic.png")`
  - `ExistingPolicy::Overwrite` -> `ResolvedTarget::Ready(".../pic.png")`
  - `ExistingPolicy::Rename` -> `ResolvedTarget::Ready(".../pic (1).png")` (and `pic (2).png` if `pic (1).png` also exists).

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --lib scanner`
Expected: FAIL (unimplemented functions)

- [ ] **Step 3: Implement `scan_inputs`, `resolve_output_dir`, and `resolve_target_path` in `src/scanner.rs`**

Implementation notes:
- Match `.webp` case-insensitively via `path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("webp"))`.
- Deduplicate using a `HashSet<PathBuf>` keyed on `std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone())`.
- When `inputs` contains a single `.webp` file directly, set `base_dir = path.parent().map(Path::to_path_buf)`. When scanning a directory `dir`, set `base_dir = Some(dir.clone())` for all files found inside it.

- [ ] **Step 4: Run test to verify pass**

Run: `cargo test --lib scanner`
Expected: PASS

- [ ] **Step 5: Commit Task 2**

```powershell
git add src/lib.rs src/scanner.rs
git commit -m "feat: add folder scanner and output path collision resolver"
```

---

### Task 3: Pure-Rust WebP Inspection & Conversion Engine (`src/converter.rs`)

**Files:**
- Create: `src/converter.rs`
- Modify: `src/lib.rs`
- Create: `tests/integration.rs`

**Interfaces:**
- Consumes: `crate::config::{AnimatedPolicy, ConversionConfig, ExistingPolicy, TargetFormat}`, `crate::scanner::{DiscoveredFile, ResolvedTarget, resolve_output_dir, resolve_target_path}`
- Produces in `webp_converter::converter`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum ConvertOutcome { Converted { outputs: Vec<PathBuf>, was_animated: bool }, Skipped { path: PathBuf, reason: String } }`
  - `pub fn inspect_webp(path: &Path) -> Result<bool, String>` (returns `Ok(true)` if animated, `Ok(false)` if static)
  - `pub fn convert_file(item: &DiscoveredFile, config: &ConversionConfig, cancel: Option<&AtomicBool>) -> Result<ConvertOutcome, String>`
  - `pub fn encode_animated_webp_for_test(frames: &[(egui::ColorImage_or_RgbaImage, u32)]) -> Vec<u8>` (or helper in `tests/integration.rs` building a valid RIFF ANIM/ANMF animated WebP from two lossless WebP frames for deterministic testing)

- [ ] **Step 1: Write failing integration tests in `tests/integration.rs`**

Write helper functions in `tests/integration.rs`:
- `create_static_webp(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf` using `image::RgbaImage` saved as `.webp`.
- `create_animated_webp(dir: &Path, name: &str) -> PathBuf` that encodes two `4x4` RGBA frames with `WebPEncoder::new_lossless`, strips the 12-byte RIFF header to get the raw `VP8L` chunk from each frame, and wraps them in a valid `RIFF....WEBP` container with `VP8X` (animation bit `0x02` set), `ANIM`, and two `ANMF` chunks (delays `50ms` and `0ms`).

Write tests in `tests/integration.rs`:
1. `auto_mode_converts_static_to_png_and_animated_to_gif`: verifies `AutoPng` creates `.png` for static and multi-frame `.gif` for animated.
2. `animated_to_static_first_frame_vs_all_frames`: verifies `TargetFormat::Png` with `FirstFrame` writes `anim.png`, and with `AllFrames` writes `anim_0001.png` and `anim_0002.png`.
3. `rgba_webp_converts_to_jpg_and_oversized_webp_converts_to_ico`: verifies transparent RGBA `.webp` converts to `.jpg` without color error, and a `300x200` `.webp` converts to `.ico` downscaled within `256x256`.
4. `delete_original_removes_on_success_and_preserves_on_corrupt_input`: verifies `delete_original = true` deletes valid source `.webp` after conversion, but keeps a corrupt `.webp` intact and leaves no `.webp-tmp` behind.

- [ ] **Step 2: Run integration tests to verify failure**

Run: `cargo test --test integration`
Expected: FAIL (missing `converter` module)

- [ ] **Step 3: Implement `src/converter.rs` and export in `src/lib.rs`**

Key implementation details in `src/converter.rs`:
- Use `image::codecs::webp::WebPDecoder::new(BufReader::new(File::open(path)?))` to inspect `decoder.has_animation()`.
- **Animated -> GIF:**
  - Collect frames via `decoder.into_frames()`. Check `cancel` flag on each frame; if cancelled, clean up temp file and return `Err("Cancelled".into())`.
  - Normalize delay: `let (num, denom) = frame.delay().numer_denom_ms(); let ms = if denom == 0 { 100 } else { num / denom }; let normalized_ms = if ms < 10 { 100 } else { ms };` and set frame delay to `Delay::from_numer_denom_ms(normalized_ms, 1)`.
  - Encode to `<target>.webp-tmp` using `GifEncoder::new_with_speed(&mut writer, config.gif_speed.clamp(1, 30))` with `set_repeat(Repeat::Infinite)`.
- **Static or Animated -> Static (`PNG`, `JPG`, `BMP`, `ICO`, or single-frame `GIF`):**
  - For `JPG`: convert `DynamicImage` to `Rgb8` by compositing alpha over opaque white (`[255, 255, 255]`) so transparent regions look clean, then encode with `JpegEncoder::new_with_quality(&mut writer, config.jpg_quality.clamp(1, 100))`.
  - For `ICO`: if `img.width() > 256 || img.height() > 256`, resize with `img.resize(256, 256, FilterType::Lanczos3)`.
  - Write to `<target>.webp-tmp`, flush, drop writer, then `std::fs::rename(&tmp_path, &final_path)`.
- If `config.delete_original` is `true`, call `std::fs::remove_file(&item.source_path)` only after all output files are renamed.

- [ ] **Step 4: Run tests to verify all integration tests pass**

Run: `cargo test`
Expected: PASS (all unit and integration tests pass)

- [ ] **Step 5: Commit Task 3**

```powershell
git add src/lib.rs src/converter.rs tests/integration.rs
git commit -m "feat: implement pure-Rust static and animated WebP conversion engine"
```

---

### Task 4: Parallel Batch Worker & Progress Channel (`src/worker.rs`)

**Files:**
- Create: `src/worker.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `crate::config::ConversionConfig`, `crate::converter::{ConvertOutcome, convert_file}`, `crate::scanner::DiscoveredFile`
- Produces in `webp_converter::worker`:
  - `#[derive(Clone, Debug)] pub enum WorkerEvent { FileStarted { index: usize }, FileCompleted { index: usize, outputs: Vec<PathBuf>, was_animated: bool, duration_ms: u64 }, FileSkipped { index: usize, path: PathBuf, reason: String }, FileFailed { index: usize, error: String }, BatchFinished { converted: usize, skipped: usize, failed: usize, cancelled: bool } }`
  - `#[derive(Clone, Debug, Default, PartialEq, Eq)] pub struct BatchSummary { pub converted: usize, pub skipped: usize, pub failed: usize, pub cancelled: bool }`
  - `pub fn run_batch_sync(items: &[DiscoveredFile], config: &ConversionConfig, cancel: &AtomicBool, on_event: impl Fn(WorkerEvent) + Send + Sync) -> BatchSummary`
  - `pub fn spawn_batch(items: Vec<DiscoveredFile>, config: ConversionConfig, on_wakeup: Option<Box<dyn Fn() + Send + Sync>>) -> (Receiver<WorkerEvent>, Arc<AtomicBool>)`

- [ ] **Step 1: Add batch worker test in `tests/integration.rs`**

Add test `parallel_batch_converts_folder_tree_and_reports_summary`:
- Creates a directory tree with 3 valid static/animated `.webp` files and 1 corrupt `.webp` file.
- Runs `run_batch_sync` with custom `output_dir`.
- Verifies `summary.converted == 3`, `summary.failed == 1`, `summary.skipped == 0`, `!summary.cancelled`, and verifies a second run with `ExistingPolicy::Skip` produces `summary.skipped == 3`.

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test integration parallel_batch`
Expected: FAIL (missing `worker` module)

- [ ] **Step 3: Implement `src/worker.rs` using `rayon::prelude::*`**

Implementation notes:
- In `run_batch_sync`, use `items.par_iter().enumerate()` and atomic counters (`AtomicUsize`) for `converted`, `skipped`, and `failed`.
- Before processing each item, check `cancel.load(Ordering::Relaxed)`. If true, skip remaining work and set `cancelled = true` in `BatchSummary`.
- Emit `BatchFinished` at the end of `run_batch_sync`.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit Task 4**

```powershell
git add src/lib.rs src/worker.rs tests/integration.rs
git commit -m "feat: add multi-threaded rayon batch worker and event stream"
```

---

### Task 5: Command-Line Interface (`src/cli.rs`)

**Files:**
- Create: `src/cli.rs`
- Modify: `src/lib.rs`
- Modify: `tests/integration.rs`

**Interfaces:**
- Consumes: `crate::config::*`, `crate::scanner::scan_inputs`, `crate::worker::{WorkerEvent, run_batch_sync}`
- Produces in `webp_converter::cli`:
  - `#[derive(Parser, Debug)] pub struct CliArgs`
  - `impl CliArgs { pub fn into_config(self) -> (Vec<PathBuf>, ConversionConfig, Option<usize>) }`
  - `pub fn run_from_args<I, T>(args: I) -> i32 where I: IntoIterator<Item = T>, T: Into<OsString> + Clone`

- [ ] **Step 1: Write CLI tests in `tests/integration.rs`**

Add test `cli_parses_flags_and_converts_directory`:
```rust
#[test]
fn cli_parses_flags_and_converts_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let in_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&in_dir).unwrap();
    create_static_webp(&in_dir, "sample.webp", 8, 8);

    let code = webp_converter::cli::run_from_args([
        "webp-converter",
        in_dir.to_str().unwrap(),
        "--format",
        "png",
        "--output",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code, 0);
    assert!(out_dir.join("sample.png").exists());
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test --test integration cli_parses_flags`
Expected: FAIL (missing `cli` module)

- [ ] **Step 3: Implement `src/cli.rs`**

CLI flags on `CliArgs`:
- `#[arg(required = true)] pub inputs: Vec<PathBuf>`
- `#[arg(short = 'f', long = "format", value_enum, default_value_t = TargetFormat::AutoPng)] pub format: TargetFormat`
- `#[arg(short = 'o', long = "output")] pub output: Option<PathBuf>`
- `#[arg(long = "no-recursive", default_value_t = false)] pub no_recursive: bool`
- `#[arg(long = "animated-policy", value_enum, default_value_t = AnimatedPolicy::FirstFrame)] pub animated_policy: AnimatedPolicy`
- `#[arg(long = "existing", value_enum, default_value_t = ExistingPolicy::Skip)] pub existing: ExistingPolicy`
- `#[arg(long = "delete-original", default_value_t = false)] pub delete_original: bool`
- `#[arg(long = "jpg-quality", default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))] pub jpg_quality: u8`
- `#[arg(long = "gif-speed", default_value_t = 10, value_parser = clap::value_parser!(i32).range(1..=30))] pub gif_speed: i32`
- `#[arg(short = 'j', long = "jobs")] pub jobs: Option<usize>`

Return exit code `0` when `discovered.len() > 0 && summary.failed == 0`, else `1`.

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit Task 5**

```powershell
git add src/lib.rs src/cli.rs tests/integration.rs
git commit -m "feat: add clap CLI parser and batch runner"
```

---

### Task 6: Desktop GUI (`src/theme.rs`, `src/chrome.rs`, `src/app.rs`, `src/main.rs`)

**Files:**
- Create: `src/theme.rs`
- Create: `src/chrome.rs`
- Create: `src/app.rs`
- Create: `src/main.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `crate::config::*`, `crate::scanner::*`, `crate::worker::*`, `crate::cli::*`
- Produces:
  - `webp_converter::theme::apply(ctx: &egui::Context)` and color constants matching `wardogs-range-calc` (`BG`, `PANEL`, `FIELD`, `LINE`, `LINE_STRONG`, `INK`, `MUTED`, `ACCENT`, `ACCENT_DIM`, `GOOD`, `BAD`)
  - `webp_converter::chrome::{TITLE_H, FRAME_INSET, WindowButton, window_frame, title_bar_drag, window_button}`
  - `webp_converter::app::{App, WINDOW_W, WINDOW_H}` implementing `eframe::App`
  - `src/main.rs` entrypoint dispatching to `cli::run_from_args(std::env::args_os())` when `std::env::args_os().len() > 1`, otherwise launching `eframe::run_native("WEBP // Converter", ...)`

- [ ] **Step 1: Write GUI harness test in `src/app.rs`**

Add a `#[cfg(test)]` module in `src/app.rs` using `egui_kittest::Harness`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn app_renders_and_queues_files() {
        let tmp = tempfile::tempdir().unwrap();
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let webp_path = tmp.path().join("test.webp");
        img.save(&webp_path).unwrap();

        let mut app = App::default();
        app.add_paths(&[webp_path]);
        assert_eq!(app.queue_len(), 1);

        let mut harness = Harness::new_eframe(|cc| {
            theme::apply(&cc.egui_ctx);
            app
        });
        harness.step();
    }
}
```

- [ ] **Step 2: Implement `src/theme.rs`, `src/chrome.rs`, `src/app.rs`, and `src/main.rs`**

Implementation notes:
- `src/theme.rs` and `src/chrome.rs`: port the HUD-style borderless frame, corner brackets, range-finder ticks, amber accent theme, and custom titlebar controls from `D:\PrivateRepos\wardogs-range-calc`.
- `src/app.rs`:
  - `WINDOW_W = 640.0`, `WINDOW_H = 780.0`.
  - Persist `ConversionConfig` and `on_top: bool` via `eframe` storage (`serde`).
  - Handle dropped files/folders from `ctx.input(|i| i.raw.dropped_files.clone())` and buttons for `+ ADD FILES` (`rfd::FileDialog::new().add_filter("WebP", &["webp"]).pick_files()`), `+ ADD FOLDER` (`rfd::FileDialog::new().pick_folder()`), and `CLEAR`.
  - Provide controls for `TargetFormat`, `AnimatedPolicy` (enabled when format is static), `ExistingPolicy`, `recursive`, `delete_original`, `jpg_quality`, `gif_speed`, and `output_dir` (`SAME AS SOURCE` or `CUSTOM FOLDER...`).
  - Poll `rx: Option<Receiver<WorkerEvent>>` each frame to update per-item status (`Ready`, `Working`, `Done { outputs, was_animated }`, `Skipped(reason)`, `Failed(error)`) and progress bar.
  - Render queue rows with `egui::ScrollArea::vertical().show_rows(ui, 26.0, self.queue.len(), ...)` so huge folder queues stay fast.
- `src/main.rs`:
  - Check `if std::env::args_os().len() > 1 { std::process::exit(webp_converter::cli::run_from_args(std::env::args_os())); }`.
  - Otherwise build `egui::ViewportBuilder` (`with_inner_size([WINDOW_W, WINDOW_H])`, `with_decorations(false)`) and call `eframe::run_native`.

- [ ] **Step 3: Run full test suite and clippy check**

Run: `cargo test`
Expected: PASS (all unit, integration, and GUI harness tests pass)

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS with 0 warnings

- [ ] **Step 4: Commit Task 6**

```powershell
git add src/lib.rs src/theme.rs src/chrome.rs src/app.rs src/main.rs
git commit -m "feat: add egui desktop GUI and dual CLI/GUI entrypoint"
```
