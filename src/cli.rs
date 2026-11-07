//! Command-line interface for batch WebP conversion.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clap::Parser;

use crate::config::{AnimatedPolicy, ConversionConfig, ExistingPolicy, TargetFormat};
use crate::scanner::scan_inputs;
use crate::worker::{WorkerEvent, run_batch_sync};

#[derive(Parser, Debug)]
#[command(
    name = "webp-converter",
    version,
    about = "Convert static and animated WebP files and folders to GIF, PNG, JPG, BMP, or ICO"
)]
pub struct CliArgs {
    /// Input .webp files or directories to convert
    #[arg(required = true)]
    pub inputs: Vec<PathBuf>,

    /// Target format (`auto-png`, `auto-jpg`, `gif`, `png`, `jpg`, `bmp`, `ico`)
    #[arg(short = 'f', long = "format", value_enum, default_value_t = TargetFormat::AutoPng)]
    pub format: TargetFormat,

    /// Custom output directory (preserves relative subfolder structure)
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Disable recursive directory scanning
    #[arg(long = "no-recursive", default_value_t = false)]
    pub no_recursive: bool,

    /// Policy when converting animated WebP to a static format (`first-frame` or `all-frames`)
    #[arg(long = "animated-policy", value_enum, default_value_t = AnimatedPolicy::FirstFrame)]
    pub animated_policy: AnimatedPolicy,

    /// Policy when output file already exists (`skip`, `overwrite`, `rename`)
    #[arg(long = "existing", value_enum, default_value_t = ExistingPolicy::Skip)]
    pub existing: ExistingPolicy,

    /// Delete original .webp file after successful conversion
    #[arg(long = "delete-original", default_value_t = false)]
    pub delete_original: bool,

    /// JPEG encoding quality (1-100)
    #[arg(long = "jpg-quality", default_value_t = 90, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub jpg_quality: u8,

    /// GIF quantization speed (1 = best quality/slowest, 30 = fastest)
    #[arg(long = "gif-speed", default_value_t = 10, value_parser = clap::value_parser!(i32).range(1..=30))]
    pub gif_speed: i32,

    /// Number of worker threads (defaults to all logical CPU cores)
    #[arg(short = 'j', long = "jobs")]
    pub jobs: Option<usize>,
}

impl CliArgs {
    pub fn into_config(self) -> (Vec<PathBuf>, ConversionConfig, Option<usize>) {
        let cfg = ConversionConfig {
            format: self.format,
            animated_policy: self.animated_policy,
            existing_policy: self.existing,
            recursive: !self.no_recursive,
            output_dir: self.output,
            delete_original: self.delete_original,
            jpg_quality: self.jpg_quality,
            gif_speed: self.gif_speed,
        };
        (self.inputs, cfg, self.jobs)
    }
}

pub fn run_from_args<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let parsed = match CliArgs::try_parse_from(args) {
        Ok(p) => p,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 1 } else { 0 };
        }
    };

    let (inputs, config, jobs) = parsed.into_config();
    let discovered = scan_inputs(&inputs, config.recursive);
    if discovered.is_empty() {
        eprintln!("No .webp files found in the specified inputs.");
        return 1;
    }

    let cancel = AtomicBool::new(false);
    let run_work = || {
        run_batch_sync(&discovered, &config, &cancel, |event| match event {
            WorkerEvent::FileCompleted {
                index,
                outputs,
                was_animated,
                duration_ms,
            } => {
                let src = &discovered[index].source_path;
                let kind = if was_animated { "ANIM" } else { "STATIC" };
                let out_display = outputs
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                println!(
                    "[OK] ({kind}) {} -> {} ({} outputs, {duration_ms}ms)",
                    src.display(),
                    out_display,
                    outputs.len()
                );
            }
            WorkerEvent::FileSkipped {
                index,
                path,
                reason,
            } => {
                let src = &discovered[index].source_path;
                println!(
                    "[SKIP] {} -> {} ({reason})",
                    src.display(),
                    path.display()
                );
            }
            WorkerEvent::FileFailed { index, error } => {
                let src = &discovered[index].source_path;
                eprintln!("[ERR] {}: {error}", src.display());
            }
            WorkerEvent::BatchFinished {
                converted,
                skipped,
                failed,
                cancelled: _,
            } => {
                println!(
                    "Done: {converted} converted, {skipped} skipped, {failed} failed (total {}).",
                    discovered.len()
                );
            }
            WorkerEvent::FileStarted { .. } => {}
        })
    };

    let summary = if let Some(num_threads) = jobs.filter(|&n| n > 0) {
        match rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
        {
            Ok(pool) => pool.install(run_work),
            Err(_) => run_work(),
        }
    } else {
        run_work()
    };

    if summary.failed == 0 && !summary.cancelled {
        0
    } else {
        1
    }
}
