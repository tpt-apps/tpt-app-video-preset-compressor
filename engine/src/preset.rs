//! The export-preset model: everything a saved "per-client export recipe"
//! captures, plus the built-in platform quick-fills.
//!
//! A preset is deliberately plain data (serde `Serialize`/`Deserialize`) so
//! the Pro edition can persist them to localStorage as JSON and the same
//! struct is what the ffmpeg argument builder consumes.

use serde::{Deserialize, Serialize};

/// Output container + its video codec. H.264/MP4 is the universal default;
/// VP9/WebM is offered for web-embed use cases where the extra encode time
/// buys smaller files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Container {
    #[serde(rename = "mp4")]
    Mp4,
    #[serde(rename = "webm")]
    WebM,
}

/// Encode difficulty knob behind three user-facing quality levels. CRF is
/// quality-targeted rather than bitrate-targeted: the same preset lands at
/// very different bitrates for talking-head vs. sports footage, which is
/// what you want when compressing for size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "balanced")]
    Balanced,
    #[serde(rename = "small")]
    Small,
}

/// Audio re-encode target. Source audio is always re-encoded (the sources
/// that reach this app are camera/phone/screen recordings whose audio is
/// rarely already AAC), so there is no "copy" mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audio {
    /// 128 kbps — music-heavy or voice-over content where clarity matters.
    #[serde(rename = "aac128")]
    Aac128,
    /// 96 kbps — default for social clips.
    #[serde(rename = "aac96")]
    Aac96,
    /// 64 kbps — talking-head maximum compression.
    #[serde(rename = "aac64")]
    Aac64,
    /// Strip audio entirely.
    #[serde(rename = "none")]
    None,
}

/// A PNG logo overlaid on every frame (the "branding overlay" from the app
/// spec). The PNG is scaled to a fixed pixel width; position is a corner
/// plus a margin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Watermark {
    /// Fraction of the video width the scaled PNG occupies, e.g. 0.15.
    pub width_fraction: f64,
    pub corner: Corner,
    /// Gap between the logo and the frame edges, in output pixels.
    pub margin_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner {
    #[serde(rename = "tl")]
    TopLeft,
    #[serde(rename = "tr")]
    TopRight,
    #[serde(rename = "bl")]
    BottomLeft,
    #[serde(rename = "br")]
    BottomRight,
}

/// One saved export recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportPreset {
    /// Client-facing name, e.g. "Acme Dental — Instagram".
    pub name: String,
    pub container: Container,
    /// Cap on output width in pixels; height follows the source aspect
    /// ratio and the clip is never upscaled. `0` keeps the original size.
    pub max_width: u16,
    /// Output frame rate; `0` keeps the source frame rate.
    pub fps: u16,
    pub quality: Quality,
    pub audio: Audio,
    #[serde(default)]
    pub watermark: Option<Watermark>,
}

impl Default for ExportPreset {
    fn default() -> Self {
        Self {
            name: String::new(),
            container: Container::Mp4,
            max_width: 0,
            fps: 0,
            quality: Quality::Balanced,
            audio: Audio::Aac96,
            watermark: None,
        }
    }
}

impl ExportPreset {
    /// File extension implied by the container.
    pub fn extension(&self) -> &'static str {
        match self.container {
            Container::Mp4 => "mp4",
            Container::WebM => "webm",
        }
    }

    /// Built-in platform quick-fills shown in the UI. These are starting
    /// points the user can override field-by-field; selecting one fills the
    /// form rather than becoming a saved preset.
    pub fn platform_presets() -> Vec<(&'static str, ExportPreset)> {
        vec![
            (
                "YouTube / landscape HD (1080p)",
                ExportPreset {
                    name: "YouTube 1080p".to_string(),
                    container: Container::Mp4,
                    max_width: 1920,
                    fps: 0,
                    quality: Quality::Balanced,
                    audio: Audio::Aac128,
                    watermark: None,
                },
            ),
            (
                "TikTok / Reels / Shorts (vertical 1080)",
                ExportPreset {
                    name: "Vertical 1080".to_string(),
                    container: Container::Mp4,
                    max_width: 1080,
                    fps: 30,
                    quality: Quality::Balanced,
                    audio: Audio::Aac96,
                    watermark: None,
                },
            ),
            (
                "Instagram feed (1080)",
                ExportPreset {
                    name: "Instagram 1080".to_string(),
                    container: Container::Mp4,
                    max_width: 1080,
                    fps: 30,
                    quality: Quality::Balanced,
                    audio: Audio::Aac96,
                    watermark: None,
                },
            ),
            (
                "X / Twitter (1280)",
                ExportPreset {
                    name: "X 1280".to_string(),
                    container: Container::Mp4,
                    max_width: 1280,
                    fps: 0,
                    quality: Quality::Balanced,
                    audio: Audio::Aac96,
                    watermark: None,
                },
            ),
            (
                "Web / email (small 960)",
                ExportPreset {
                    name: "Web small 960".to_string(),
                    container: Container::Mp4,
                    max_width: 960,
                    fps: 30,
                    quality: Quality::Small,
                    audio: Audio::Aac64,
                    watermark: None,
                },
            ),
            // No WebM/VP9 quick-fill: the current @ffmpeg/core build's
            // libvpx-vp9 traps its wasm memory mid-encode ("memory access
            // out of bounds"), so VP9 is not offered to users. Revisit when
            // vendoring a newer core (see assets/README.md).
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_balanced_mp4_original_size() {
        let p = ExportPreset::default();
        assert_eq!(p.container, Container::Mp4);
        assert_eq!(p.max_width, 0);
        assert_eq!(p.fps, 0);
        assert_eq!(p.quality, Quality::Balanced);
        assert_eq!(p.audio, Audio::Aac96);
        assert_eq!(p.watermark, None);
        assert_eq!(p.extension(), "mp4");
    }

    #[test]
    fn json_round_trip_preserves_every_field() {
        let preset = ExportPreset {
            name: "Acme Dental — Instagram".to_string(),
            container: Container::WebM,
            max_width: 1080,
            fps: 30,
            quality: Quality::Small,
            audio: Audio::Aac64,
            watermark: Some(Watermark {
                width_fraction: 0.15,
                corner: Corner::BottomRight,
                margin_px: 24,
            }),
        };
        let json = serde_json::to_string(&preset).unwrap();
        let back: ExportPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preset);
    }

    #[test]
    fn json_round_trip_without_watermark() {
        let preset = ExportPreset::default();
        let json = serde_json::to_string(&preset).unwrap();
        let back: ExportPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, preset);
    }

    #[test]
    fn platform_presets_have_distinct_names() {
        let presets = ExportPreset::platform_presets();
        assert!(presets.len() >= 5);
        let mut names: Vec<&str> = presets.iter().map(|(label, _)| *label).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), presets.len());
    }
}
