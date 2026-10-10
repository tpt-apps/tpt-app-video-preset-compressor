//! The export-preset model: everything a saved "per-client export recipe"
//! captures, plus the built-in platform quick-fills.
//!
//! A preset is deliberately plain data (serde `Serialize`/`Deserialize`) so
//! the Pro edition can persist them to localStorage as JSON and
//! [`encode::EncodeSpec`](crate::encode::EncodeSpec) is built straight from it.

use serde::{Deserialize, Serialize};

/// Output container + its video codec. Royalty-free output only: WebM with
/// VP8 video and Opus audio. H.264/AAC are deliberately not offered — they
/// are patent-encumbered and this app ships no codec licences.
///
/// Presets saved by earlier builds say `"mp4"`; that alias loads as WebM so
/// they keep working instead of failing to deserialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Container {
    #[serde(rename = "webm", alias = "mp4")]
    WebM,
}

/// Video codec for the WebM output. All three are royalty-free and encoded
/// by the browser's own WebCodecs encoders (the engine falls back down the
/// list when the preferred one isn't available).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum VideoCodec {
    /// Smallest files, slowest encode.
    #[serde(rename = "av1")]
    Av1,
    /// Best balance of size and speed; the default.
    #[default]
    #[serde(rename = "vp9")]
    Vp9,
    /// Fastest, most compatible, largest files.
    #[serde(rename = "vp8")]
    Vp8,
}

/// Size/quality trade-off behind three user-facing levels. Each level is a
/// video bitrate budget (bits per pixel per frame, see `assets/engine.js`),
/// so output size follows the clip's resolution and frame rate. The levels
/// are deliberately not the browser encoder's own quality presets, which
/// produce files larger than a typical H.264 source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "high")]
    High,
    #[serde(rename = "balanced")]
    Balanced,
    #[serde(rename = "small")]
    Small,
}

/// Audio re-encode target (Opus). Source audio is always re-encoded (the
/// sources that reach this app are camera/phone/screen recordings whose audio
/// is AAC, which can't go in WebM), so there is no "copy" mode. Presets saved
/// by earlier builds say `"aac*"`; those load as the same bitrate of Opus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Audio {
    /// 128 kbps — music-heavy or voice-over content where clarity matters.
    #[serde(rename = "opus128", alias = "aac128")]
    Opus128,
    /// 96 kbps — default for social clips.
    #[serde(rename = "opus96", alias = "aac96")]
    Opus96,
    /// 64 kbps — talking-head maximum compression.
    #[serde(rename = "opus64", alias = "aac64")]
    Opus64,
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
    /// Presets saved by earlier builds have no codec; they get the default.
    #[serde(default)]
    pub codec: VideoCodec,
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
            container: Container::WebM,
            codec: VideoCodec::default(),
            max_width: 0,
            fps: 0,
            quality: Quality::Balanced,
            audio: Audio::Opus96,
            watermark: None,
        }
    }
}

impl ExportPreset {
    /// File extension implied by the container.
    pub fn extension(&self) -> &'static str {
        match self.container {
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
                    container: Container::WebM,
                    codec: VideoCodec::default(),
                    max_width: 1920,
                    fps: 0,
                    quality: Quality::Balanced,
                    audio: Audio::Opus128,
                    watermark: None,
                },
            ),
            (
                "TikTok / Reels / Shorts (vertical 1080)",
                ExportPreset {
                    name: "Vertical 1080".to_string(),
                    container: Container::WebM,
                    codec: VideoCodec::default(),
                    max_width: 1080,
                    fps: 30,
                    quality: Quality::Balanced,
                    audio: Audio::Opus96,
                    watermark: None,
                },
            ),
            (
                "Instagram feed (1080)",
                ExportPreset {
                    name: "Instagram 1080".to_string(),
                    container: Container::WebM,
                    codec: VideoCodec::default(),
                    max_width: 1080,
                    fps: 30,
                    quality: Quality::Balanced,
                    audio: Audio::Opus96,
                    watermark: None,
                },
            ),
            (
                "X / Twitter (1280)",
                ExportPreset {
                    name: "X 1280".to_string(),
                    container: Container::WebM,
                    codec: VideoCodec::default(),
                    max_width: 1280,
                    fps: 0,
                    quality: Quality::Balanced,
                    audio: Audio::Opus96,
                    watermark: None,
                },
            ),
            (
                "Web / email (small 960)",
                ExportPreset {
                    name: "Web small 960".to_string(),
                    container: Container::WebM,
                    codec: VideoCodec::default(),
                    max_width: 960,
                    fps: 30,
                    quality: Quality::Small,
                    audio: Audio::Opus64,
                    watermark: None,
                },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_balanced_webm_original_size() {
        let p = ExportPreset::default();
        assert_eq!(p.container, Container::WebM);
        assert_eq!(p.max_width, 0);
        assert_eq!(p.fps, 0);
        assert_eq!(p.quality, Quality::Balanced);
        assert_eq!(p.audio, Audio::Opus96);
        assert_eq!(p.watermark, None);
        assert_eq!(p.extension(), "webm");
    }

    #[test]
    fn json_round_trip_preserves_every_field() {
        let preset = ExportPreset {
            name: "Acme Dental — Instagram".to_string(),
            container: Container::WebM,
            codec: VideoCodec::Av1,
            max_width: 1080,
            fps: 30,
            quality: Quality::Small,
            audio: Audio::Opus64,
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
    fn legacy_mp4_aac_presets_load_as_webm_opus() {
        let json = r#"{"name":"Old","container":"mp4","max_width":1080,"fps":30,
            "quality":"small","audio":"aac64","watermark":null}"#;
        let p: ExportPreset = serde_json::from_str(json).unwrap();
        assert_eq!(p.container, Container::WebM);
        assert_eq!(p.audio, Audio::Opus64);
        // No codec field in old presets -> the default (VP9).
        assert_eq!(p.codec, VideoCodec::Vp9);
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
