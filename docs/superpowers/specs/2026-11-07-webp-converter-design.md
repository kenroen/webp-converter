# WebP Converter — Design Specification

**Date:** 2026-11-07  
**Project:** `webp-converter` (`D:\PrivateRepos\webp-converter`)  
**Language & Toolchain:** Rust 2024 Edition (`rust-version = "1.90"`), `#![forbid(unsafe_code)]`

---

## 1. Overview & Goals

`webp-converter` is a standalone, pure-Rust desktop and command-line application for converting static and animated `.webp` files—either individually or across entire directory trees—into `.gif` or static image formats (`.png`, `.jpg`, `.bmp`, `.ico`).

### Key Objectives
1. **Dual Interface (GUI + CLI) in a Single Binary:**
   - Launching with no command-line arguments opens an `eframe`/`egui` desktop GUI styled consistently with `D:\PrivateRepos\wardogs-range-calc` (custom frameless window chrome, dark field-equipment theme with amber accents, persistent user settings).
   - Launching with arguments runs in scriptable CLI mode with terminal output and exit codes.
2. **Smart Animated & Static WebP Handling:**
   - Automatically detects whether each `.webp` file is static or animated via `image::codecs::webp::WebPDecoder::has_animation()`.
   - Supports **Auto Mode** (Static → PNG or JPEG; Animated → GIF) as well as **Explicit Target Mode** (`GIF`, `PNG`, `JPEG`, `BMP`, `ICO`).
   - Supports configurable behavior when an animated `.webp` is explicitly converted to a static format: either **First Frame Only** or **Extract All Frames** (`<stem>_0001.<ext>`, `<stem>_0002.<ext>`, ...).
3. **Full Folder & Batch Processing:**
   - Supports converting entire folders (recursively or top-level only).
   - Outputs alongside original files by default, or into a custom output directory while preserving relative subdirectory structure.
   - Configurable collision handling (`Skip`, `Overwrite`, `Rename`) and optional safe deletion of the original `.webp` after verified conversion.
   - Multi-threaded batch execution via `rayon` with real-time progress reporting and cancellation support.

---

## 2. Architecture & Module Structure

```text
webp-converter/
├── Cargo.toml
├── src/
│   ├── main.rs        # Entrypoint: dispatches to CLI if args present, otherwise launches eframe GUI
│   ├── lib.rs         # Exports core modules for integration testing
│   ├── config.rs      # Shared configuration types (TargetFormat, AnimatedPolicy, ExistingPolicy, etc.)
│   ├── scanner.rs     # File & folder discovery, relative path calculation, output path resolution
│   ├── converter.rs   # Pure-Rust WebP inspection, decoding, frame extraction, and target encoding
│   ├── worker.rs      # Rayon parallel batch runner, progress channel events, cancellation token
│   ├── cli.rs         # Clap CLI parser, console progress reporter, exit code handling
│   ├── app.rs         # egui/eframe application state, drag-and-drop, settings UI, queue view
│   ├── chrome.rs      # Frameless window titlebar, resize grips, and window controls
│   └── theme.rs       # Dark field-equipment visual style and color palette
└── tests/
    └── integration.rs # End-to-end conversion, folder hierarchy, collision, and animated WebP tests
```

---

## 3. Core Data Model (`src/config.rs`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum TargetFormat {
    /// Static -> PNG, Animated -> GIF
    AutoPng,
    /// Static -> JPG, Animated -> GIF
    AutoJpg,
    Gif,
    Png,
    Jpg,
    Bmp,
    Ico,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum AnimatedPolicy {
    /// Save only the first frame when converting an animated WebP to a static format
    FirstFrame,
    /// Save all frames as <stem>_0001.<ext>, <stem>_0002.<ext>, ...
    AllFrames,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum ExistingPolicy {
    /// Do not overwrite existing output files; mark job as Skipped
    Skip,
    /// Replace existing output files atomically
    Overwrite,
    /// Append counter: <stem> (1).<ext>, <stem> (2).<ext>, ...
    Rename,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionConfig {
    pub format: TargetFormat,
    pub animated_policy: AnimatedPolicy,
    pub existing_policy: ExistingPolicy,
    pub recursive: bool,
    pub output_dir: Option<PathBuf>,
    pub delete_original: bool,
    pub jpg_quality: u8, // 1..=100, default 90
    pub gif_speed: i32,  // 1..=30, default 10
}
```

---

## 4. Folder Discovery & Path Resolution (`src/scanner.rs`)

### Input Scanning
Given a list of input paths (files and/or directories):
1. If an input path is a file with a `.webp` extension (case-insensitive), its `source_path` is recorded with `base_dir = file.parent()`.
2. If an input path is a directory:
   - Walk using `walkdir::WalkDir`.
   - If `config.recursive == false`, set `max_depth(1)`.
   - For every `.webp` file discovered inside `dir`, record `source_path` and `base_dir = Some(dir)`.
3. Deduplicate discovered files by canonical/normalized path while preserving discovery order.

### Output Path Resolution
For each `DiscoveredFile { source_path, base_dir }` and resolved output extension `ext`:
1. **Target Directory:**
   - If `config.output_dir` is `None`: use `source_path.parent()`.
   - If `config.output_dir` is `Some(out_root)`:
     - Compute `rel_dir = source_path.parent().strip_prefix(base_dir)`.
     - Target directory is `out_root.join(rel_dir)`.
2. **Collision Handling (`ExistingPolicy`):**
   - Construct candidate path `<target_dir>/<stem>.<ext>` (or `<stem>_0001.<ext>` for multi-frame extraction).
   - If candidate exists:
     - `Skip`: return `ConvertOutcome::Skipped`.
     - `Overwrite`: write to temporary file and replace candidate.
     - `Rename`: find first unused `<target_dir>/<stem> (<n>).<ext>` (`n >= 1`).

---

## 5. Conversion Engine (`src/converter.rs`)

### Step 1: WebP Inspection
Open `source_path` with `BufReader<File>` and construct `image::codecs::webp::WebPDecoder::new(reader)`:
- Query `decoder.has_animation()`.
- Determine effective output action based on `(has_animation, config.format, config.animated_policy)`:

| Source WebP | `TargetFormat` | `AnimatedPolicy` | Effective Action |
|---|---|---|---|
| Static | `AutoPng` or `Png` | Any | Encode single `PNG` |
| Static | `AutoJpg` or `Jpg` | Any | Encode single `JPG` (`jpg_quality`) |
| Static | `Gif` | Any | Encode single-frame `GIF` |
| Static | `Bmp` / `Ico` | Any | Encode single `BMP` / `ICO` |
| Animated | `AutoPng` / `AutoJpg` / `Gif` | Any | Decode all frames → Encode animated `GIF` |
| Animated | `Png` / `Jpg` / `Bmp` / `Ico` | `FirstFrame` | Decode first frame → Encode single static image |
| Animated | `Png` / `Jpg` / `Bmp` / `Ico` | `AllFrames` | Decode all frames → Encode `<stem>_0001.<ext>` .. `<stem>_NNNN.<ext>` |

### Step 2: Encoding Details & Edge Cases
1. **Animated GIF Encoding:**
   - Iterate frames via `decoder.into_frames()`.
   - Normalize frame delay: if a frame's delay is `0ms`, normalize to standard `100ms` (10 FPS fallback); otherwise preserve the `Delay` numerator/denominator in 10ms GIF ticks.
   - Use `image::codecs::gif::GifEncoder::new_with_speed(&mut temp_writer, config.gif_speed)` and `encoder.set_repeat(Repeat::Infinite)`.
2. **JPEG Alpha Compositing:**
   - WebP images often have an alpha channel (`Rgba8`), whereas JPEG only supports RGB (`Rgb8`).
   - When encoding to JPEG, composite `Rgba8` pixels over an opaque neutral dark/white background (or convert to `Rgb8`) before passing to `JpegEncoder::new_with_quality(&mut temp_writer, config.jpg_quality)`.
3. **ICO Dimension Constraints:**
   - ICO format supports dimensions up to `256x256`. If converting to `ICO` and `width > 256` or `height > 256`, downscale proportionally using `FilterType::Lanczos3` to fit within `256x256`.
4. **Atomic Writes & Safe Source Deletion:**
   - Every output file is first written to `<output_path>.webp-tmp` in the destination directory.
   - Once encoding flushes cleanly, rename `<output_path>.webp-tmp` to `<output_path>`.
   - If an error or cancellation occurs mid-write, remove `<output_path>.webp-tmp`.
   - If `config.delete_original == true`, `std::fs::remove_file(source_path)` is called **only after** all output files for that source have been flushed and atomically renamed.

---

## 6. Parallel Batch Worker (`src/worker.rs`)

- Runs on a background thread spawning a `rayon` parallel iterator over the discovered file list.
- Checks `cancel_flag: Arc<AtomicBool>` before starting each file and between frames of animated WebPs.
- Sends `WorkerEvent` messages over `std::sync::mpsc::Sender<WorkerEvent>`:
  - `FileStarted { index: usize }`
  - `FileCompleted { index: usize, outputs: Vec<PathBuf>, duration_ms: u64 }`
  - `FileSkipped { index: usize, reason: String }`
  - `FileFailed { index: usize, error: String }`
  - `BatchFinished { converted: usize, skipped: usize, failed: usize, cancelled: bool }`

---

## 7. User Interfaces

### 7.1 Desktop GUI (`src/app.rs`, `src/chrome.rs`, `src/theme.rs`)
- **Window Dimensions:** `640.0 x 780.0` fixed/resizable frameless window with custom titlebar (`WEBP // CONVERTER`), pin-on-top toggle, minimize, and close buttons.
- **Sections:**
  1. **Drop Zone & Input Controls:**
     - Drag-and-drop area accepting files and folders from Windows Explorer (`ctx.input(|i| i.raw.dropped_files)`).
     - Buttons: `+ ADD FILES`, `+ ADD FOLDER`, `CLEAR ALL`.
  2. **Configuration Panel:**
     - Format selector (`AUTO (PNG + GIF)`, `AUTO (JPG + GIF)`, `GIF`, `PNG`, `JPG`, `BMP`, `ICO`).
     - Animated-to-static policy selector (`FIRST FRAME`, `ALL FRAMES`) when a static format is forced.
     - Sliders for `JPG QUALITY` (`1..=100`) and `GIF SPEED` (`1..=30`).
     - Toggles: `RECURSIVE SUBFOLDERS`, `DELETE ORIGINAL WEBP`.
     - Existing file policy: `SKIP`, `OVERWRITE`, `RENAME`.
     - Output folder picker: `SAME AS SOURCE` or `CUSTOM FOLDER...` with browse/reset buttons.
  3. **Queue & Progress Panel:**
     - Progress bar (`converted + skipped + failed / total`) and summary counters.
     - Primary action button: `CONVERT ALL (N)` (changes to `CANCEL` while active).
     - Scrollable virtualized queue list (`egui::ScrollArea::vertical().show_rows(...)` so 10,000+ files render smoothly at 60 FPS) displaying filename, detected/output badge, and status (`READY`, `WORKING`, `DONE`, `SKIP`, `ERR`).

### 7.2 Command-Line Interface (`src/cli.rs`)
- Activated whenever `std::env::args_os().len() > 1`.
- On Windows, calls a safe console output routine or standard `stdout`/`stderr` (using standard console subsystem or dual-mode output) and supports:
  ```text
  webp-converter <INPUTS>... [OPTIONS]
  ```
- Options:
  - `-f, --format <FORMAT>` (`auto-png`, `auto-jpg`, `gif`, `png`, `jpg`, `bmp`, `ico`) [default: `auto-png`]
  - `-o, --output <DIR>`
  - `--no-recursive` (recursive is enabled by default in CLI mode when a directory is passed)
  - `--animated-policy <first-frame|all-frames>` [default: `first-frame`]
  - `--existing <skip|overwrite|rename>` [default: `skip`]
  - `--delete-original`
  - `--jpg-quality <1-100>` [default: `90`]
  - `--gif-speed <1-30>` [default: `10`]
  - `-j, --jobs <N>` (optional thread count)
- Exits with code `0` if all files succeeded or were skipped, or `1` if any file failed or no `.webp` files were found.

---

## 8. Testing Strategy

1. **Programmatic Fixture Generation:**
   - Tests generate valid static RGBA `.webp`, static RGB `.webp`, and multi-frame animated `.webp` buffers in temporary directories so no large binary test assets need to be committed.
2. **Unit & Integration Tests (`tests/integration.rs`):**
   - Verify `AutoPng` and `AutoJpg` convert static `.webp` → `.png`/`.jpg` and animated `.webp` → multi-frame `.gif`.
   - Verify explicit `Png` with `AnimatedPolicy::FirstFrame` vs `AnimatedPolicy::AllFrames`.
   - Verify recursive directory scanning preserves nested folder hierarchy inside custom `output_dir`.
   - Verify `ExistingPolicy::Skip`, `Overwrite`, and `Rename`.
   - Verify `delete_original` deletes source `.webp` on success and preserves source `.webp` when decoding fails on corrupt input.
3. **GUI Harness Tests:**
   - Verify `App` initializes cleanly and renders via `egui_kittest`.
