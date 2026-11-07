use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::webp::WebPEncoder;
use image::{AnimationDecoder, ExtendedColorType, ImageEncoder, Rgba, RgbaImage};
use webp_converter::config::{AnimatedPolicy, ConversionConfig, ExistingPolicy, TargetFormat};
use webp_converter::converter::{ConvertOutcome, convert_file, inspect_webp};
use webp_converter::scanner::DiscoveredFile;

pub fn create_static_webp(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
    let mut img = RgbaImage::new(width, height);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = Rgba([(x % 255) as u8, (y % 255) as u8, 128, 200]);
    }
    let path = dir.join(name);
    img.save(&path).unwrap();
    path
}

fn encode_lossless_webp_chunk(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = WebPEncoder::new_lossless(Cursor::new(&mut buf));
    encoder
        .write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgba8)
        .unwrap();
    // Strip the 12-byte "RIFF<size>WEBP" header to keep the inner VP8L chunk (with its 8-byte header + payload).
    buf[12..].to_vec()
}

pub fn create_animated_webp(dir: &Path, name: &str) -> PathBuf {
    let width = 4u32;
    let height = 4u32;
    let frame1 = RgbaImage::from_pixel(width, height, Rgba([255, 0, 0, 255]));
    let frame2 = RgbaImage::from_pixel(width, height, Rgba([0, 255, 0, 128]));

    let chunk1 = encode_lossless_webp_chunk(&frame1);
    let chunk2 = encode_lossless_webp_chunk(&frame2);

    let mut payload = Vec::new();

    // VP8X chunk: 10 bytes
    payload.extend_from_slice(b"VP8X");
    payload.extend_from_slice(&10u32.to_le_bytes());
    // Flags: animation (0x02) | alpha (0x10) = 0x12
    payload.extend_from_slice(&[0x12, 0x00, 0x00, 0x00]);
    // Canvas width - 1 (24-bit LE)
    let w_minus_1 = width - 1;
    payload.extend_from_slice(&w_minus_1.to_le_bytes()[..3]);
    // Canvas height - 1 (24-bit LE)
    let h_minus_1 = height - 1;
    payload.extend_from_slice(&h_minus_1.to_le_bytes()[..3]);

    // ANIM chunk: 6 bytes (bg color RGBA + loop count u16)
    payload.extend_from_slice(b"ANIM");
    payload.extend_from_slice(&6u32.to_le_bytes());
    payload.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

    for (chunk, duration_ms) in [(&chunk1, 50u32), (&chunk2, 0u32)] {
        let mut anmf_data = Vec::new();
        // Frame X (3 bytes), Frame Y (3 bytes)
        anmf_data.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        // Frame Width - 1 (3 bytes)
        anmf_data.extend_from_slice(&w_minus_1.to_le_bytes()[..3]);
        // Frame Height - 1 (3 bytes)
        anmf_data.extend_from_slice(&h_minus_1.to_le_bytes()[..3]);
        // Frame Duration (3 bytes LE)
        anmf_data.extend_from_slice(&duration_ms.to_le_bytes()[..3]);
        // Flags (1 byte): 0x02 (do not blend)
        anmf_data.push(0x02);
        // Frame data (VP8L chunk)
        anmf_data.extend_from_slice(chunk);

        payload.extend_from_slice(b"ANMF");
        payload.extend_from_slice(&(anmf_data.len() as u32).to_le_bytes());
        payload.extend_from_slice(&anmf_data);
        if anmf_data.len() % 2 == 1 {
            payload.push(0);
        }
    }

    let mut riff = Vec::new();
    riff.extend_from_slice(b"RIFF");
    riff.extend_from_slice(&((4 + payload.len()) as u32).to_le_bytes());
    riff.extend_from_slice(b"WEBP");
    riff.extend_from_slice(&payload);

    let path = dir.join(name);
    std::fs::write(&path, riff).unwrap();
    path
}

#[test]
fn auto_mode_converts_static_to_png_and_animated_to_gif() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let static_path = create_static_webp(dir, "static.webp", 8, 8);
    let anim_path = create_animated_webp(dir, "anim.webp");

    assert!(!inspect_webp(&static_path).unwrap());
    assert!(inspect_webp(&anim_path).unwrap());

    let cfg = ConversionConfig::default(); // AutoPng

    let out_static = convert_file(
        &DiscoveredFile {
            source_path: static_path,
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg,
        None,
    )
    .unwrap();
    assert_eq!(
        out_static,
        ConvertOutcome::Converted {
            outputs: vec![dir.join("static.png")],
            was_animated: false,
        }
    );
    assert!(dir.join("static.png").exists());

    let out_anim = convert_file(
        &DiscoveredFile {
            source_path: anim_path,
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg,
        None,
    )
    .unwrap();
    let gif_path = dir.join("anim.gif");
    assert_eq!(
        out_anim,
        ConvertOutcome::Converted {
            outputs: vec![gif_path.clone()],
            was_animated: true,
        }
    );
    assert!(gif_path.exists());

    let gif_file = std::fs::File::open(&gif_path).unwrap();
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(gif_file)).unwrap();
    let frames: Vec<_> = decoder.into_frames().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(frames.len(), 2);
}

#[test]
fn animated_to_static_first_frame_vs_all_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let anim_path = create_animated_webp(dir, "clip.webp");

    let cfg_first = ConversionConfig {
        format: TargetFormat::Png,
        animated_policy: AnimatedPolicy::FirstFrame,
        ..ConversionConfig::default()
    };
    let res_first = convert_file(
        &DiscoveredFile {
            source_path: anim_path.clone(),
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg_first,
        None,
    )
    .unwrap();
    assert_eq!(
        res_first,
        ConvertOutcome::Converted {
            outputs: vec![dir.join("clip.png")],
            was_animated: true,
        }
    );
    assert!(dir.join("clip.png").exists());

    let cfg_all = ConversionConfig {
        format: TargetFormat::Png,
        animated_policy: AnimatedPolicy::AllFrames,
        ..ConversionConfig::default()
    };
    let res_all = convert_file(
        &DiscoveredFile {
            source_path: anim_path,
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg_all,
        None,
    )
    .unwrap();
    assert_eq!(
        res_all,
        ConvertOutcome::Converted {
            outputs: vec![dir.join("clip_0001.png"), dir.join("clip_0002.png")],
            was_animated: true,
        }
    );
    assert!(dir.join("clip_0001.png").exists());
    assert!(dir.join("clip_0002.png").exists());
}

#[test]
fn rgba_webp_converts_to_jpg_and_oversized_webp_converts_to_ico() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let big_webp = create_static_webp(dir, "big.webp", 300, 200);

    let cfg_jpg = ConversionConfig {
        format: TargetFormat::Jpg,
        ..ConversionConfig::default()
    };
    convert_file(
        &DiscoveredFile {
            source_path: big_webp.clone(),
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg_jpg,
        None,
    )
    .unwrap();
    assert!(dir.join("big.jpg").exists());

    let cfg_ico = ConversionConfig {
        format: TargetFormat::Ico,
        ..ConversionConfig::default()
    };
    convert_file(
        &DiscoveredFile {
            source_path: big_webp,
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg_ico,
        None,
    )
    .unwrap();
    let ico_img = image::open(dir.join("big.ico")).unwrap();
    assert!(ico_img.width() <= 256 && ico_img.height() <= 256);
}

#[test]
fn delete_original_removes_on_success_and_preserves_on_corrupt_input() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let valid = create_static_webp(dir, "valid.webp", 8, 8);
    let corrupt = dir.join("corrupt.webp");
    std::fs::write(&corrupt, b"not a valid webp file").unwrap();

    let cfg = ConversionConfig {
        format: TargetFormat::Png,
        delete_original: true,
        existing_policy: ExistingPolicy::Overwrite,
        ..ConversionConfig::default()
    };

    let ok_res = convert_file(
        &DiscoveredFile {
            source_path: valid.clone(),
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg,
        None,
    );
    assert!(ok_res.is_ok());
    assert!(!valid.exists());
    assert!(dir.join("valid.png").exists());

    let err_res = convert_file(
        &DiscoveredFile {
            source_path: corrupt.clone(),
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg,
        None,
    );
    assert!(err_res.is_err());
    assert!(corrupt.exists());
}

#[test]
fn parallel_batch_converts_folder_tree_and_reports_summary() {
    use std::sync::atomic::AtomicBool;
    use webp_converter::scanner::scan_inputs;
    use webp_converter::worker::run_batch_sync;

    let tmp = tempfile::tempdir().unwrap();
    let in_root = tmp.path().join("input");
    let sub = in_root.join("nested");
    let out_root = tmp.path().join("output");
    std::fs::create_dir_all(&sub).unwrap();

    create_static_webp(&in_root, "one.webp", 8, 8);
    create_static_webp(&sub, "two.webp", 8, 8);
    create_animated_webp(&sub, "three.webp");
    std::fs::write(sub.join("broken.webp"), b"corrupt webp bytes").unwrap();

    let items = scan_inputs(&[in_root], true);
    assert_eq!(items.len(), 4);

    let cfg = ConversionConfig {
        format: TargetFormat::AutoPng,
        existing_policy: ExistingPolicy::Skip,
        output_dir: Some(out_root.clone()),
        ..ConversionConfig::default()
    };

    let cancel = AtomicBool::new(false);
    let summary = run_batch_sync(&items, &cfg, &cancel, |_| {});
    assert_eq!(summary.converted, 3);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.skipped, 0);
    assert!(!summary.cancelled);

    assert!(out_root.join("one.png").exists());
    assert!(out_root.join("nested").join("two.png").exists());
    assert!(out_root.join("nested").join("three.gif").exists());

    // Running a second time with ExistingPolicy::Skip skips the 3 already-converted files
    let summary2 = run_batch_sync(&items, &cfg, &cancel, |_| {});
    assert_eq!(summary2.converted, 0);
    assert_eq!(summary2.skipped, 3);
    assert_eq!(summary2.failed, 1);
}

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

#[test]
fn test_all_frames_skip_on_later_existing_frame_preserves_original_and_rolls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let anim_path = create_animated_webp(dir, "partial.webp");
    // Pre-create frame 2 while frame 1 does not exist
    std::fs::write(dir.join("partial_0002.png"), b"existing frame 2").unwrap();

    let cfg = ConversionConfig {
        format: TargetFormat::Png,
        animated_policy: AnimatedPolicy::AllFrames,
        existing_policy: ExistingPolicy::Skip,
        delete_original: true,
        ..ConversionConfig::default()
    };

    let outcome = convert_file(
        &DiscoveredFile {
            source_path: anim_path.clone(),
            base_dir: Some(dir.to_path_buf()),
        },
        &cfg,
        None,
    )
    .unwrap();

    assert!(matches!(outcome, ConvertOutcome::Skipped { .. }));
    assert!(anim_path.exists(), "original animated webp must not be deleted when a frame is skipped");
    assert!(!dir.join("partial_0001.png").exists(), "partially written earlier frames should be cleaned up on skip");
}

#[test]
fn test_same_stem_files_from_different_folders_into_shared_output_dir() {
    use std::sync::atomic::AtomicBool;
    use webp_converter::scanner::scan_inputs;
    use webp_converter::worker::run_batch_sync;

    let tmp = tempfile::tempdir().unwrap();
    let out_dir = tmp.path().join("out");
    let mut inputs = Vec::new();
    for i in 0..8 {
        let d = tmp.path().join(format!("dir_{i}"));
        std::fs::create_dir_all(&d).unwrap();
        create_static_webp(&d, "collision.webp", 16, 16);
        inputs.push(d);
    }

    let items = scan_inputs(&inputs, true);
    assert_eq!(items.len(), 8);

    let cfg = ConversionConfig {
        format: TargetFormat::Png,
        existing_policy: ExistingPolicy::Rename,
        output_dir: Some(out_dir.clone()),
        ..ConversionConfig::default()
    };

    let cancel = AtomicBool::new(false);
    let summary = run_batch_sync(&items, &cfg, &cancel, |_| {});
    assert_eq!(summary.converted, 8);
    assert_eq!(summary.failed, 0);
    assert!(out_dir.join("collision.png").exists());
    for i in 1..8 {
        assert!(
            out_dir.join(format!("collision ({i}).png")).exists(),
            "missing collision ({i}).png"
        );
    }
}
