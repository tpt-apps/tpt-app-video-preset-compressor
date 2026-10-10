//! Media metadata reported by the JS engine's `probe()` (`assets/engine.js`).

use serde::Deserialize;

/// What the app needs to know about a clip before compressing it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MediaInfo {
    /// Container duration in seconds (fractional).
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    /// Average frame rate; `0.0` when unknown.
    #[serde(default)]
    pub fps: f64,
    /// e.g. "avc", "hevc", "vp9", "av1" (WebCodecs/mediabunny naming).
    pub video_codec: String,
    /// Whether this browser can decode the video stream.
    #[serde(default = "yes")]
    pub can_decode_video: bool,
    /// `None` for clips with no audio track.
    #[serde(default)]
    pub audio: Option<AudioInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AudioInfo {
    /// e.g. "aac", "opus", "mp3".
    pub codec: String,
    pub sample_rate_hz: u32,
    pub channels: u32,
    #[serde(default = "yes")]
    pub can_decode: bool,
}

fn yes() -> bool {
    true
}

impl AudioInfo {
    /// "mono", "stereo" or "N ch".
    pub fn channel_label(&self) -> String {
        match self.channels {
            1 => "mono".to_string(),
            2 => "stereo".to_string(),
            n => format!("{n} ch"),
        }
    }
}

impl MediaInfo {
    /// Parses the JSON object returned by the JS `probe()`.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_probe_json_with_audio() {
        let info = MediaInfo::from_json(
            r#"{"duration_secs":6.0,"width":1280,"height":720,"fps":30,
                "video_codec":"avc","can_decode_video":true,
                "audio":{"codec":"aac","sample_rate_hz":44100,"channels":2,"can_decode":true}}"#,
        )
        .unwrap();
        assert_eq!(info.width, 1280);
        assert_eq!(info.video_codec, "avc");
        let audio = info.audio.unwrap();
        assert_eq!(audio.channel_label(), "stereo");
    }

    #[test]
    fn no_audio_track_is_none_and_missing_fields_default() {
        let info = MediaInfo::from_json(
            r#"{"duration_secs":1,"width":640,"height":360,"video_codec":"vp9","audio":null}"#,
        )
        .unwrap();
        assert!(info.audio.is_none());
        assert_eq!(info.fps, 0.0);
        assert!(info.can_decode_video);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(MediaInfo::from_json("not json").is_err());
    }
}
