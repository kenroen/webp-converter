//! Shared configuration types for WebP conversion.

use std::path::PathBuf;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum TargetFormat {
    /// Static -> PNG, Animated -> GIF
    #[default]
    #[value(name = "auto-png")]
    AutoPng,
    /// Static -> JPG, Animated -> GIF
    #[value(name = "auto-jpg")]
    AutoJpg,
    #[value(name = "gif")]
    Gif,
    #[value(name = "png")]
    Png,
    #[value(name = "jpg")]
    Jpg,
    #[value(name = "bmp")]
    Bmp,
    #[value(name = "ico")]
    Ico,
}

impl TargetFormat {
    pub const ALL: [TargetFormat; 7] = [
        TargetFormat::AutoPng,
        TargetFormat::AutoJpg,
        TargetFormat::Gif,
        TargetFormat::Png,
        TargetFormat::Jpg,
        TargetFormat::Bmp,
        TargetFormat::Ico,
    ];

    pub fn resolve_extension(self, is_animated: bool) -> &'static str {
        match (self, is_animated) {
            (TargetFormat::AutoPng, false) => "png",
            (TargetFormat::AutoPng, true) => "gif",
            (TargetFormat::AutoJpg, false) => "jpg",
            (TargetFormat::AutoJpg, true) => "gif",
            (TargetFormat::Gif, _) => "gif",
            (TargetFormat::Png, _) => "png",
            (TargetFormat::Jpg, _) => "jpg",
            (TargetFormat::Bmp, _) => "bmp",
            (TargetFormat::Ico, _) => "ico",
        }
    }

    pub fn is_gif_for(self, is_animated: bool) -> bool {
        matches!(
            (self, is_animated),
            (TargetFormat::Gif, _)
                | (TargetFormat::AutoPng, true)
                | (TargetFormat::AutoJpg, true)
        )
    }

    pub fn is_explicit_static(self) -> bool {
        matches!(
            self,
            TargetFormat::Png | TargetFormat::Jpg | TargetFormat::Bmp | TargetFormat::Ico
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            TargetFormat::AutoPng => "AUTO (PNG/GIF)",
            TargetFormat::AutoJpg => "AUTO (JPG/GIF)",
            TargetFormat::Gif => "GIF",
            TargetFormat::Png => "PNG",
            TargetFormat::Jpg => "JPG",
            TargetFormat::Bmp => "BMP",
            TargetFormat::Ico => "ICO",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum AnimatedPolicy {
    /// Save only the first frame when converting an animated WebP to a static format
    #[default]
    #[value(name = "first-frame")]
    FirstFrame,
    /// Save all frames as <stem>_0001.<ext>, <stem>_0002.<ext>, ...
    #[value(name = "all-frames")]
    AllFrames,
}

impl AnimatedPolicy {
    pub fn label(self) -> &'static str {
        match self {
            AnimatedPolicy::FirstFrame => "FIRST FRAME",
            AnimatedPolicy::AllFrames => "ALL FRAMES",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum ExistingPolicy {
    /// Do not overwrite existing output files; mark job as Skipped
    #[default]
    #[value(name = "skip")]
    Skip,
    /// Replace existing output files atomically
    #[value(name = "overwrite")]
    Overwrite,
    /// Append counter: <stem> (1).<ext>, <stem> (2).<ext>, ...
    #[value(name = "rename")]
    Rename,
}

impl ExistingPolicy {
    pub fn label(self) -> &'static str {
        match self {
            ExistingPolicy::Skip => "SKIP",
            ExistingPolicy::Overwrite => "OVERWRITE",
            ExistingPolicy::Rename => "RENAME",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConversionConfig {
    pub format: TargetFormat,
    pub animated_policy: AnimatedPolicy,
    pub existing_policy: ExistingPolicy,
    pub recursive: bool,
    pub output_dir: Option<PathBuf>,
    pub delete_original: bool,
    pub jpg_quality: u8,
    pub gif_speed: i32,
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            format: TargetFormat::AutoPng,
            animated_policy: AnimatedPolicy::FirstFrame,
            existing_policy: ExistingPolicy::Skip,
            recursive: true,
            output_dir: None,
            delete_original: false,
            jpg_quality: 90,
            gif_speed: 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_format_resolves_auto_and_explicit_extensions() {
        assert_eq!(TargetFormat::AutoPng.resolve_extension(false), "png");
        assert_eq!(TargetFormat::AutoPng.resolve_extension(true), "gif");
        assert_eq!(TargetFormat::AutoJpg.resolve_extension(false), "jpg");
        assert_eq!(TargetFormat::AutoJpg.resolve_extension(true), "gif");
        assert_eq!(TargetFormat::Png.resolve_extension(true), "png");
        assert_eq!(TargetFormat::Gif.resolve_extension(false), "gif");
        assert!(TargetFormat::AutoPng.is_gif_for(true));
        assert!(!TargetFormat::AutoPng.is_gif_for(false));
        assert!(TargetFormat::Gif.is_gif_for(false));
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
}
