//! Pure-Rust WebP inspection, decoding, and target format encoding.

use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use image::codecs::bmp::BmpEncoder;
use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::ico::IcoEncoder;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{
    AnimationDecoder, Delay, DynamicImage, ExtendedColorType, Frame, ImageEncoder, Rgb, RgbImage,
};

use crate::config::{AnimatedPolicy, ConversionConfig, ExistingPolicy};
use crate::scanner::{DiscoveredFile, ResolvedTarget, resolve_output_dir, resolve_target_path};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);
static FINALIZE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConvertOutcome {
    Converted {
        outputs: Vec<PathBuf>,
        was_animated: bool,
    },
    Skipped {
        path: PathBuf,
        reason: String,
    },
}

fn check_cancelled(cancel: Option<&AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        Err("Cancelled".to_string())
    } else {
        Ok(())
    }
}

pub fn inspect_webp(path: &Path) -> Result<bool, String> {
    let file = File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let decoder = WebPDecoder::new(BufReader::new(file))
        .map_err(|e| format!("Invalid WebP {}: {e}", path.display()))?;
    Ok(decoder.has_animation())
}

fn unique_temp_path(target_dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    target_dir.join(format!(".{stem}.{ext}.{id}.webp-tmp"))
}

fn composite_rgba_to_rgb_white(img: &DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut rgb = RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = u16::from(p[3]);
        let inv_a = 255 - a;
        let r = ((u16::from(p[0]) * a + 255 * inv_a) / 255) as u8;
        let g = ((u16::from(p[1]) * a + 255 * inv_a) / 255) as u8;
        let b = ((u16::from(p[2]) * a + 255 * inv_a) / 255) as u8;
        rgb.put_pixel(x, y, Rgb([r, g, b]));
    }
    rgb
}

fn ensure_dir(dir: &Path) -> Result<(), String> {
    if !dir.as_os_str().is_empty() && !dir.is_dir() {
        for attempt in 0..3 {
            match std::fs::create_dir_all(dir) {
                Ok(()) => return Ok(()),
                Err(_) if dir.is_dir() => return Ok(()),
                Err(_) if attempt < 2 => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => {
                    return Err(format!("Failed to create directory {}: {e}", dir.display()));
                }
            }
        }
    }
    Ok(())
}

fn finalize_temp_file(
    tmp_path: &Path,
    target_dir: &Path,
    stem: &str,
    ext: &str,
    policy: ExistingPolicy,
) -> Result<ResolvedTarget, String> {
    let _guard = FINALIZE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match resolve_target_path(target_dir, stem, ext, policy) {
        ResolvedTarget::SkipExisting(existing) => {
            let _ = std::fs::remove_file(tmp_path);
            Ok(ResolvedTarget::SkipExisting(existing))
        }
        ResolvedTarget::Ready(final_path) => {
            std::fs::rename(tmp_path, &final_path).map_err(|e| {
                let _ = std::fs::remove_file(tmp_path);
                format!("Failed to finalize {}: {e}", final_path.display())
            })?;
            Ok(ResolvedTarget::Ready(final_path))
        }
    }
}

fn write_static_image_atomic(
    img: &DynamicImage,
    target_dir: &Path,
    stem: &str,
    ext: &str,
    config: &ConversionConfig,
) -> Result<ResolvedTarget, String> {
    ensure_dir(target_dir)?;

    let tmp_path = unique_temp_path(target_dir, stem, ext);
    let result = (|| -> Result<(), String> {
        let file = File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp file {}: {e}", tmp_path.display()))?;
        let mut writer = BufWriter::new(file);

        match ext {
            "png" => {
                let rgba = img.to_rgba8();
                PngEncoder::new(&mut writer)
                    .write_image(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                        ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| format!("PNG encode error: {e}"))?;
            }
            "jpg" => {
                let rgb = composite_rgba_to_rgb_white(img);
                let quality = config.jpg_quality.clamp(1, 100);
                JpegEncoder::new_with_quality(&mut writer, quality)
                    .write_image(
                        rgb.as_raw(),
                        rgb.width(),
                        rgb.height(),
                        ExtendedColorType::Rgb8,
                    )
                    .map_err(|e| format!("JPG encode error: {e}"))?;
            }
            "bmp" => {
                let rgba = img.to_rgba8();
                BmpEncoder::new(&mut writer)
                    .write_image(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                        ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| format!("BMP encode error: {e}"))?;
            }
            "ico" => {
                let resized = if img.width() > 256 || img.height() > 256 {
                    img.resize(256, 256, FilterType::Lanczos3)
                } else {
                    img.clone()
                };
                let rgba = resized.to_rgba8();
                IcoEncoder::new(&mut writer)
                    .write_image(
                        rgba.as_raw(),
                        rgba.width(),
                        rgba.height(),
                        ExtendedColorType::Rgba8,
                    )
                    .map_err(|e| format!("ICO encode error: {e}"))?;
            }
            "gif" => {
                let rgba = img.to_rgba8();
                let speed = config.gif_speed.clamp(1, 30);
                let mut encoder = GifEncoder::new_with_speed(&mut writer, speed);
                let frame = Frame::new(rgba);
                encoder
                    .encode_frame(frame)
                    .map_err(|e| format!("GIF encode error: {e}"))?;
            }
            other => return Err(format!("Unsupported output extension: {other}")),
        }

        writer
            .flush()
            .map_err(|e| format!("Failed to flush output: {e}"))?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    finalize_temp_file(&tmp_path, target_dir, stem, ext, config.existing_policy)
}

fn normalize_frame_delay(delay: Delay) -> Delay {
    let (num, denom) = delay.numer_denom_ms();
    let ms = num.checked_div(denom).unwrap_or(100);
    let normalized_ms = if ms < 10 { 100 } else { ms };
    Delay::from_numer_denom_ms(normalized_ms, 1)
}

fn write_animated_gif_atomic(
    source_path: &Path,
    target_dir: &Path,
    stem: &str,
    config: &ConversionConfig,
    cancel: Option<&AtomicBool>,
) -> Result<ResolvedTarget, String> {
    ensure_dir(target_dir)?;

    let tmp_path = unique_temp_path(target_dir, stem, "gif");
    let result = (|| -> Result<(), String> {
        let src_file = File::open(source_path)
            .map_err(|e| format!("Failed to open {}: {e}", source_path.display()))?;
        let decoder = WebPDecoder::new(BufReader::new(src_file))
            .map_err(|e| format!("WebP decode error: {e}"))?;

        let dst_file = File::create(&tmp_path)
            .map_err(|e| format!("Failed to create temp file {}: {e}", tmp_path.display()))?;
        let mut writer = BufWriter::new(dst_file);

        let speed = config.gif_speed.clamp(1, 30);
        let mut encoder = GifEncoder::new_with_speed(&mut writer, speed);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| format!("GIF repeat header error: {e}"))?;

        for frame_res in decoder.into_frames() {
            check_cancelled(cancel)?;
            let frame = frame_res.map_err(|e| format!("Animated WebP frame decode error: {e}"))?;
            let delay = normalize_frame_delay(frame.delay());
            let buffer = frame.into_buffer();
            let normalized_frame = Frame::from_parts(buffer, 0, 0, delay);
            encoder
                .encode_frame(normalized_frame)
                .map_err(|e| format!("Animated GIF frame encode error: {e}"))?;
        }

        drop(encoder);
        writer
            .flush()
            .map_err(|e| format!("Failed to flush GIF output: {e}"))?;
        Ok(())
    })();

    if let Err(err) = result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    finalize_temp_file(&tmp_path, target_dir, stem, "gif", config.existing_policy)
}

pub fn convert_file(
    item: &DiscoveredFile,
    config: &ConversionConfig,
    cancel: Option<&AtomicBool>,
) -> Result<ConvertOutcome, String> {
    check_cancelled(cancel)?;

    let is_animated = inspect_webp(&item.source_path)?;
    let ext = config.format.resolve_extension(is_animated);
    let target_dir = resolve_output_dir(item, config.output_dir.as_deref());
    let stem = item
        .source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("Invalid filename: {}", item.source_path.display()))?;

    let mut written_outputs = Vec::new();

    if is_animated && config.format.is_gif_for(true) {
        if let ResolvedTarget::SkipExisting(existing) =
            resolve_target_path(&target_dir, stem, "gif", config.existing_policy)
        {
            return Ok(ConvertOutcome::Skipped {
                path: existing,
                reason: "Output file already exists".to_string(),
            });
        }
        match write_animated_gif_atomic(&item.source_path, &target_dir, stem, config, cancel)? {
            ResolvedTarget::SkipExisting(existing) => {
                return Ok(ConvertOutcome::Skipped {
                    path: existing,
                    reason: "Output file already exists".to_string(),
                });
            }
            ResolvedTarget::Ready(target_path) => {
                written_outputs.push(target_path);
            }
        }
    } else if is_animated && config.animated_policy == AnimatedPolicy::AllFrames {
        let src_file = File::open(&item.source_path)
            .map_err(|e| format!("Failed to open {}: {e}", item.source_path.display()))?;
        let decoder = WebPDecoder::new(BufReader::new(src_file))
            .map_err(|e| format!("WebP decode error: {e}"))?;

        for (idx, frame_res) in decoder.into_frames().enumerate() {
            if let Err(e) = check_cancelled(cancel) {
                for p in &written_outputs {
                    let _ = std::fs::remove_file(p);
                }
                return Err(e);
            }
            let frame = match frame_res {
                Ok(f) => f,
                Err(e) => {
                    for p in &written_outputs {
                        let _ = std::fs::remove_file(p);
                    }
                    return Err(format!("Frame {} decode error: {e}", idx + 1));
                }
            };
            let frame_stem = format!("{stem}_{:04}", idx + 1);
            if let ResolvedTarget::SkipExisting(existing) =
                resolve_target_path(&target_dir, &frame_stem, ext, config.existing_policy)
            {
                for p in &written_outputs {
                    let _ = std::fs::remove_file(p);
                }
                return Ok(ConvertOutcome::Skipped {
                    path: existing,
                    reason: "Output frame already exists".to_string(),
                });
            }
            let dyn_img = DynamicImage::ImageRgba8(frame.into_buffer());
            match write_static_image_atomic(&dyn_img, &target_dir, &frame_stem, ext, config) {
                Ok(ResolvedTarget::SkipExisting(existing)) => {
                    for p in &written_outputs {
                        let _ = std::fs::remove_file(p);
                    }
                    return Ok(ConvertOutcome::Skipped {
                        path: existing,
                        reason: "Output frame already exists".to_string(),
                    });
                }
                Ok(ResolvedTarget::Ready(target_path)) => {
                    written_outputs.push(target_path);
                }
                Err(e) => {
                    for p in &written_outputs {
                        let _ = std::fs::remove_file(p);
                    }
                    return Err(e);
                }
            }
        }
    } else {
        if let ResolvedTarget::SkipExisting(existing) =
            resolve_target_path(&target_dir, stem, ext, config.existing_policy)
        {
            return Ok(ConvertOutcome::Skipped {
                path: existing,
                reason: "Output file already exists".to_string(),
            });
        }

        let dyn_img = if is_animated {
            let src_file = File::open(&item.source_path)
                .map_err(|e| format!("Failed to open {}: {e}", item.source_path.display()))?;
            let decoder = WebPDecoder::new(BufReader::new(src_file))
                .map_err(|e| format!("WebP decode error: {e}"))?;
            let first_frame = decoder
                .into_frames()
                .next()
                .ok_or_else(|| "Animated WebP contained no frames".to_string())?
                .map_err(|e| format!("First frame decode error: {e}"))?;
            DynamicImage::ImageRgba8(first_frame.into_buffer())
        } else {
            image::open(&item.source_path)
                .map_err(|e| format!("Failed to decode {}: {e}", item.source_path.display()))?
        };

        check_cancelled(cancel)?;
        match write_static_image_atomic(&dyn_img, &target_dir, stem, ext, config)? {
            ResolvedTarget::SkipExisting(existing) => {
                return Ok(ConvertOutcome::Skipped {
                    path: existing,
                    reason: "Output file already exists".to_string(),
                });
            }
            ResolvedTarget::Ready(target_path) => {
                written_outputs.push(target_path);
            }
        }
    }

    if config.delete_original && !written_outputs.is_empty() {
        std::fs::remove_file(&item.source_path).map_err(|e| {
            format!(
                "Converted successfully, but failed to delete original {}: {e}",
                item.source_path.display()
            )
        })?;
    }

    Ok(ConvertOutcome::Converted {
        outputs: written_outputs,
        was_animated: is_animated,
    })
}
