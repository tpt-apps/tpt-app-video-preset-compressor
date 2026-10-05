//! Reads media metadata out of `ffmpeg -i <input>` log output.
//!
//! Probing this way (rather than via ffprobe) works because the metadata is
//! written to the same stderr stream the ffmpeg.wasm logger already hands
//! us, costs one near-instant run (no output file requested → ffmpeg exits
//! after printing input info), and keeps this crate's parsing host-testable
//! against plain strings.

/// What the app needs to know about a clip before compressing it.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    /// Container duration in seconds (fractional).
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    /// Average frame rate from the container, e.g. 29.97.
    pub fps: f64,
    /// e.g. "h264", "vp9", "hevc".
    pub video_codec: String,
    /// `None` for clips with no audio track.
    pub audio: Option<AudioInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    /// e.g. "aac", "opus", "pcm_s16le".
    pub codec: String,
    pub sample_rate_hz: u32,
    /// "mono", "stereo", "5.1", … as ffmpeg prints it.
    pub channels: String,
}

/// Parses the stderr text of a `ffmpeg -hide_banner -i input` run. Missing
/// fields (a clip with no audio, no parseable fps…) degrade to safe
/// defaults rather than failing the whole probe; a log with no video
/// stream at all returns `None` (not a video file).
pub fn parse_probe_log(log: &str) -> Option<MediaInfo> {
    let mut duration_secs = 0.0;
    let mut video: Option<(u32, u32, f64, String)> = None;
    let mut audio: Option<AudioInfo> = None;

    for line in log.lines() {
        let trimmed = line.trim_start();
        if duration_secs == 0.0 {
            if let Some(rest) = trimmed.strip_prefix("Duration:") {
                if let Some(d) = parse_hhmmss(rest.trim_start()) {
                    duration_secs = d;
                }
            }
        }
        if video.is_none() {
            if let Some(rest) = stream_after(trimmed, "Video:") {
                video = parse_video_stream(rest);
            }
        }
        if audio.is_none() {
            if let Some(rest) = stream_after(trimmed, "Audio:") {
                audio = parse_audio_stream(rest);
            }
        }
    }

    let (width, height, fps, video_codec) = video?;
    Some(MediaInfo {
        duration_secs,
        width,
        height,
        fps,
        video_codec,
        audio,
    })
}

/// Matches `Stream #0:0[0x1](und): Video: …` / `Stream #0:1…: Audio: …` and
/// returns everything after the `Video:`/`Audio:` marker.
fn stream_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    if !line.starts_with("Stream #") {
        return None;
    }
    let pos = line.find(marker)?;
    Some(line[pos + marker.len()..].trim())
}

/// `00:01:23.45, start: 0.000000, bitrate: …` → 83.45
fn parse_hhmmss(text: &str) -> Option<f64> {
    let head = text.split(',').next()?.trim();
    let mut parts = head.split(':');
    let h: f64 = parts.next()?.trim().parse().ok()?;
    let m: f64 = parts.next()?.trim().parse().ok()?;
    let s: f64 = parts.next()?.trim().parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + s)
}

/// `h264 (High 4:4:4 Predictive) (avc1 / 0x31637661), yuv444p(…), 640x360
/// [SAR 1:1 DAR 16:9], 221 kb/s, 30 fps, 30 tbr, …` → codec/dims/fps.
fn parse_video_stream(text: &str) -> Option<(u32, u32, f64, String)> {
    // Codec name: first whitespace-delimited token.
    let codec = text.split_whitespace().next()?.to_string();

    // Dimensions: the first `WxH` token anywhere in the stream line (the
    // pixel-format token right before it can contain letters but never an
    // embedded `x` between digits).
    let (width, height) = text
        .split(|c: char| c == ',' || c == ' ')
        .find_map(|tok| parse_wxh(tok.trim()))?;

    // Frame rate: prefer `N fps`, fall back to `N tbr`.
    let fps = find_number_unit(text, "fps").or_else(|| find_number_unit(text, "tbr"))?;

    Some((width, height, fps, codec))
}

/// `aac (LC) (mp4a / 0x6134706D), 44100 Hz, mono, fltp, 96 kb/s` → info.
fn parse_audio_stream(text: &str) -> Option<AudioInfo> {
    let codec = text.split_whitespace().next()?.to_string();
    let sample_rate_hz = find_number_unit(text, "Hz")? as u32;
    let channels = text
        .split(',')
        .find_map(|part| {
            let p = part.trim();
            for known in ["mono", "stereo", "5.1", "7.1", "quad"] {
                if p == known {
                    return Some(p.to_string());
                }
            }
            None
        })
        .unwrap_or_else(|| "unknown".to_string());
    Some(AudioInfo {
        codec,
        sample_rate_hz,
        channels,
    })
}

fn parse_wxh(token: &str) -> Option<(u32, u32)> {
    let (w, h) = token.split_once('x')?;
    let width = w.trim().parse().ok()?;
    let height = h.trim().parse().ok()?;
    (width > 0 && height > 0 && width <= 100_000 && height <= 100_000).then_some((width, height))
}

/// Finds the number in front of a unit token, e.g. `… 30 fps, 30 tbr, …`
/// with unit `"fps"` → 30.0. ffmpeg separates values and units with a
/// space and may trail a comma after the unit (`44100 Hz,`).
fn find_number_unit(text: &str, unit: &str) -> Option<f64> {
    let mut prev: Option<&str> = None;
    for token in text.split_whitespace() {
        if token.trim_end_matches(',') == unit {
            if let Some(value) = prev.and_then(|p| p.trim_end_matches(',').parse().ok()) {
                return Some(value);
            }
        }
        prev = Some(token);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `ffmpeg -hide_banner -i` output (captured from a local
    /// ffmpeg build) for an H.264/AAC MP4.
    const SAMPLE_MP4_LOG: &str = "\
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'sample.mp4':
  Metadata:
    major_brand     : isom
    minor_version   : 512
    compatible_brands: isomiso2avc1mp41
    encoder         : Lavf60.20.100
  Duration: 00:00:02.00, start: 0.000000, bitrate: 331 kb/s
  Stream #0:0[0x1](und): Video: h264 (High 4:4:4 Predictive) (avc1 / 0x31637661), yuv444p(progressive), 640x360 [SAR 1:1 DAR 16:9], 221 kb/s, 30 fps, 30 tbr, 15360 tbn (default)
      Metadata:
        handler_name    : VideoHandler
        vendor_id       : [0][0][0][0]
        encoder         : Lavc60.36.100 libx264
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, mono, fltp, 96 kb/s (default)
      Metadata:
        handler_name    : SoundHandler
        vendor_id       : [0][0][0][0]
At least one output file must be specified";

    /// Typical phone-clip shape: HEVC 4K 29.97 tbr without an `fps` field,
    /// stereo 48 kHz audio.
    const PHONE_MOV_LOG: &str = "\
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'IMG_0001.mov':
  Duration: 00:03:27.16, start: 0.000000, bitrate: 51243 kb/s
  Stream #0:0[0x1](und): Video: hevc (Main) (hvc1 / 0x31637668), yuv420p(tv, bt709), 3840x2160, 51105 kb/s, 29.97 tbr, 600 tbn (default)
  Stream #0:1[0x2](und): Audio: aac (LC) (mp4a / 0x6134706D), 44100 Hz, stereo, fltp, 187 kb/s (default)";

    #[test]
    fn parses_real_h264_aac_mp4_probe() {
        let info = parse_probe_log(SAMPLE_MP4_LOG).expect("video stream should be found");
        assert!((info.duration_secs - 2.0).abs() < 1e-9);
        assert_eq!(info.width, 640);
        assert_eq!(info.height, 360);
        assert!((info.fps - 30.0).abs() < 1e-9);
        assert_eq!(info.video_codec, "h264");
        let audio = info.audio.expect("audio stream should be found");
        assert_eq!(audio.codec, "aac");
        assert_eq!(audio.sample_rate_hz, 44100);
        assert_eq!(audio.channels, "mono");
    }

    #[test]
    fn parses_hevc_phone_clip_with_tbr_fallback() {
        let info = parse_probe_log(PHONE_MOV_LOG).expect("video stream should be found");
        assert!((info.duration_secs - 207.16).abs() < 1e-9);
        assert_eq!(info.width, 3840);
        assert_eq!(info.height, 2160);
        // No `fps` token — falls back to tbr.
        assert!((info.fps - 29.97).abs() < 1e-9);
        assert_eq!(info.video_codec, "hevc");
        assert_eq!(info.audio.unwrap().channels, "stereo");
    }

    #[test]
    fn no_audio_track_degrades_to_none() {
        let log = "\
Input #0, mov,mp4,m4a,3gp,3g2,mj2, from 'silent.mp4':
  Duration: 00:00:10.00, start: 0.000000, bitrate: 1200 kb/s
  Stream #0:0[0x1](und): Video: h264 (High) (avc1 / 0x31637661), yuv420p, 1920x1080 [SAR 1:1 DAR 16:9], 1200 kb/s, 25 fps, 25 tbr, 12800 tbn";
        let info = parse_probe_log(log).unwrap();
        assert_eq!(info.audio, None);
        assert_eq!((info.width, info.height), (1920, 1080));
    }

    #[test]
    fn non_video_log_returns_none() {
        let log = "\
Input #0, wav, from 'audio.wav':
  Duration: 00:00:05.00, start: 0.000000, bitrate: 1411 kb/s
  Stream #0:0: Audio: pcm_s16le ([1][0][0][0] / 0x0001), 44100 Hz, 2 channels, s16, 1411 kb/s";
        assert_eq!(parse_probe_log(log), None);
    }

    #[test]
    fn garbage_returns_none() {
        assert_eq!(parse_probe_log("total nonsense"), None);
        assert_eq!(parse_probe_log(""), None);
    }

    #[test]
    fn duration_variants() {
        assert!((parse_hhmmss("00:00:02.00, start: 0.000000").unwrap() - 2.0).abs() < 1e-9);
        assert!((parse_hhmmss("01:02:03.5").unwrap() - 3723.5).abs() < 1e-9);
        assert_eq!(parse_hhmmss("not a duration"), None);
    }
}
