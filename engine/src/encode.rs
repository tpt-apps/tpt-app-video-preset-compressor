//! Turns an [`ExportPreset`] into the [`EncodeSpec`] the JS engine
//! (`assets/engine.js`, `compress()`) consumes.
//!
//! Everything here is royalty-free by construction: video is AV1 / VP9 /
//! VP8 and audio is Opus, written to WebM by the browser's own WebCodecs
//! encoders. There is no way to express H.264 or AAC in this model.

use serde::Serialize;

use crate::preset::{Audio, Corner, ExportPreset, Quality, VideoCodec, Watermark};

/// The JSON object passed to `engine.js`'s `compress()`. Field names match
/// that function's `spec` parameter.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EncodeSpec {
    pub codec: &'static str,
    pub quality: &'static str,
    /// Output width cap in pixels; `0` keeps the source width. Never upscales.
    pub max_width: u16,
    /// Output frame rate; `0` keeps the source frame rate.
    pub fps: u16,
    /// Opus bitrate in bits/s; `0` strips audio.
    pub audio_bitrate: u32,
    pub watermark: Option<WatermarkSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WatermarkSpec {
    /// Fraction of the *output* width the logo occupies.
    pub fraction: f64,
    pub corner: &'static str,
    pub margin: u32,
}

impl ExportPreset {
    /// Builds the spec for the JS engine. A watermark in the preset is only
    /// included when the caller actually has PNG bytes to supply (`has_png`);
    /// otherwise the plain path is used rather than producing a broken job.
    pub fn to_spec(&self, has_png: bool) -> EncodeSpec {
        EncodeSpec {
            codec: match self.codec {
                VideoCodec::Av1 => "av1",
                VideoCodec::Vp9 => "vp9",
                VideoCodec::Vp8 => "vp8",
            },
            quality: match self.quality {
                Quality::High => "high",
                Quality::Balanced => "balanced",
                Quality::Small => "small",
            },
            max_width: self.max_width,
            fps: self.fps,
            audio_bitrate: self.audio.bitrate(),
            watermark: match (self.watermark.as_ref(), has_png) {
                (Some(wm), true) => Some(watermark_spec(wm)),
                _ => None,
            },
        }
    }
}

fn watermark_spec(wm: &Watermark) -> WatermarkSpec {
    WatermarkSpec {
        fraction: wm.width_fraction,
        corner: match wm.corner {
            Corner::TopLeft => "tl",
            Corner::TopRight => "tr",
            Corner::BottomLeft => "bl",
            Corner::BottomRight => "br",
        },
        margin: wm.margin_px,
    }
}

impl Audio {
    /// Opus bitrate in bits/s; `0` means "strip audio".
    pub fn bitrate(self) -> u32 {
        match self {
            Audio::Opus128 => 128_000,
            Audio::Opus96 => 96_000,
            Audio::Opus64 => 64_000,
            Audio::None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preset_is_vp9_balanced_opus96() {
        let spec = ExportPreset::default().to_spec(false);
        assert_eq!(spec.codec, "vp9");
        assert_eq!(spec.quality, "balanced");
        assert_eq!(spec.max_width, 0);
        assert_eq!(spec.audio_bitrate, 96_000);
        assert_eq!(spec.watermark, None);
    }

    #[test]
    fn audio_none_means_zero_bitrate() {
        let spec = ExportPreset { audio: Audio::None, ..ExportPreset::default() }.to_spec(false);
        assert_eq!(spec.audio_bitrate, 0);
    }

    #[test]
    fn every_codec_choice_is_royalty_free() {
        for codec in [VideoCodec::Av1, VideoCodec::Vp9, VideoCodec::Vp8] {
            let spec = ExportPreset { codec, ..ExportPreset::default() }.to_spec(false);
            assert!(["av1", "vp9", "vp8"].contains(&spec.codec));
        }
    }

    #[test]
    fn watermark_needs_png_bytes() {
        let preset = ExportPreset {
            watermark: Some(Watermark {
                width_fraction: 0.15,
                corner: Corner::BottomRight,
                margin_px: 16,
            }),
            ..ExportPreset::default()
        };
        assert_eq!(preset.to_spec(false).watermark, None);
        let wm = preset.to_spec(true).watermark.unwrap();
        assert_eq!((wm.corner, wm.margin), ("br", 16));
        assert!((wm.fraction - 0.15).abs() < 1e-9);
    }

    #[test]
    fn spec_serializes_with_the_field_names_engine_js_reads() {
        let json = serde_json::to_value(ExportPreset::default().to_spec(false)).unwrap();
        for key in ["codec", "quality", "max_width", "fps", "audio_bitrate", "watermark"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
