//! Turns an [`ExportPreset`] into the ffmpeg.wasm argument vector.
//!
//! Everything here must stay in sync with what the vendored `@ffmpeg/core`
//! 0.12.10 build actually supports: H.264 via libx264, VP9 via
//! libvpx-vp9, AAC via the native encoder, PNG input via the image2
//! demuxer, and the scale/overlay filters. No text/font filters (the core
//! has no usable fontconfig) — branding overlays are PNG-only.

use crate::preset::{Audio, Container, Corner, ExportPreset, Quality, Watermark};

/// Fixed in-memory filename the app writes a preset's watermark PNG to.
pub const WATERMARK_FILENAME: &str = "watermark.png";

/// Builds the full argument vector for one compression run. `input` and
/// `output` are virtual in-memory filenames inside the ffmpeg.wasm virtual
/// FS (the app uses fixed names like `input.mp4` / `output.webm`).
///
/// When the preset carries a watermark, pass `watermark_width_px` — the
/// resolved overlay width (the app computes it from the probed source
/// width, since the core's libavfilter is too old to do `main_w` math
/// inside `scale`) — and the PNG must have been written to
/// [`WATERMARK_FILENAME`].
pub fn build_ffmpeg_args(
    preset: &ExportPreset,
    input: &str,
    output: &str,
    watermark_width_px: Option<u32>,
) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".to_string(), "-i".to_string(), input.to_string()];

    // The filter chain: scale (+ fps) on the main video, then the optional
    // watermark overlay. With a watermark we must use -filter_complex (two
    // inputs); without one, a plain -vf keeps the command minimal.
    match (preset.watermark.as_ref(), watermark_width_px) {
        (Some(wm), Some(wm_width)) => {
            args.push("-i".to_string());
            args.push(WATERMARK_FILENAME.to_string());
            args.push("-filter_complex".to_string());
            args.push(watermarked_filter_chain(preset, wm, wm_width));
            args.push("-map".to_string());
            args.push("[vout]".to_string());
            // Map the source audio when the preset keeps it; `0:a?` is
            // optional so clips with no audio track still encode.
            if preset.audio != Audio::None {
                args.push("-map".to_string());
                args.push("0:a?".to_string());
            }
        }
        _ => {
            if let Some(scale) = scale_filter(preset) {
                args.push("-vf".to_string());
                args.push(scale);
            }
            if preset.fps > 0 {
                args.push("-r".to_string());
                args.push(preset.fps.to_string());
            }
        }
    }

    match preset.container {
        Container::Mp4 => {
            args.push("-c:v".to_string());
            args.push("libx264".to_string());
            let (crf, speed) = h264_quality(preset.quality);
            args.push("-crf".to_string());
            args.push(crf.to_string());
            args.push("-preset".to_string());
            args.push(speed.to_string());
            // Player compatibility: 8-bit 4:2:0 plays everywhere (phone
            // browsers, PowerPoint, old smart TVs).
            args.push("-pix_fmt".to_string());
            args.push("yuv420p".to_string());
            // Move the index to the front so playback starts before the
            // whole file is downloaded — matters for the hub preview.
            args.push("-movflags".to_string());
            args.push("+faststart".to_string());
        }
        Container::WebM => {
            args.push("-c:v".to_string());
            args.push("libvpx-vp9".to_string());
            args.push("-crf".to_string());
            args.push(vp9_quality(preset.quality).to_string());
            // -b:v 0 = constant-quality mode for VP9 (CRF alone is ignored).
            args.push("-b:v".to_string());
            args.push("0".to_string());
            args.push("-pix_fmt".to_string());
            args.push("yuv420p".to_string());
        }
    }

    match preset.audio {
        Audio::None => args.push("-an".to_string()),
        Audio::Aac128 => push_audio(&mut args, preset.container, 128_000),
        Audio::Aac96 => push_audio(&mut args, preset.container, 96_000),
        Audio::Aac64 => push_audio(&mut args, preset.container, 64_000),
    }

    args.push(output.to_string());
    args
}

/// Watermark width in output pixels for a given source width: the preset's
/// fraction of the *final* width (after the width cap), floored to a
/// sane minimum so a 0.15 fraction of a tiny clip can't vanish.
pub fn resolve_watermark_width(preset: &ExportPreset, source_width: u32) -> u32 {
    let final_width = match preset.max_width {
        0 => source_width,
        cap => source_width.min(cap as u32),
    };
    let fraction = preset
        .watermark
        .as_ref()
        .map(|wm| wm.width_fraction)
        .unwrap_or(0.15);
    ((final_width as f64 * fraction).round() as u32).max(8)
}

/// The scale fragment: cap width, keep aspect ratio, never upscale, and
/// land on even dimensions (`-2`) so yuv420p chroma subsampling stays legal.
fn scale_filter(preset: &ExportPreset) -> Option<String> {
    match preset.max_width {
        0 => None,
        w => Some(format!("scale='min(iw,{w})':-2")),
    }
}

/// `filter_complex` chain for presets with a watermark: scale the main
/// video, scale the logo to `watermark_width_px`, overlay it in the chosen
/// corner, and expose the result as `[vout]`.
fn watermarked_filter_chain(
    preset: &ExportPreset,
    wm: &Watermark,
    watermark_width_px: u32,
) -> String {
    let scale_bit = scale_filter(preset)
        .map(|s| format!("[0:v]{s}[v0];"))
        .unwrap_or_else(|| "[0:v]null[v0];".to_string());
    let (x, y) = match wm.corner {
        Corner::TopLeft => (wm.margin_px.to_string(), wm.margin_px.to_string()),
        Corner::TopRight => (
            format!("main_w-overlay_w-{}", wm.margin_px),
            wm.margin_px.to_string(),
        ),
        Corner::BottomLeft => (
            wm.margin_px.to_string(),
            format!("main_h-overlay_h-{}", wm.margin_px),
        ),
        Corner::BottomRight => (
            format!("main_w-overlay_w-{}", wm.margin_px),
            format!("main_h-overlay_h-{}", wm.margin_px),
        ),
    };
    format!("{scale_bit}[1:v]scale={watermark_width_px}:-1[wm];[v0][wm]overlay={x}:{y}[vout]")
}

fn h264_quality(q: Quality) -> (u8, &'static str) {
    match q {
        Quality::High => (18, "veryfast"),
        Quality::Balanced => (23, "veryfast"),
        Quality::Small => (28, "veryfast"),
    }
}

fn vp9_quality(q: Quality) -> u8 {
    match q {
        Quality::High => 30,
        Quality::Balanced => 33,
        Quality::Small => 38,
    }
}

fn push_audio(args: &mut Vec<String>, container: Container, bitrate: u32) {
    match container {
        Container::Mp4 => {
            args.push("-c:a".to_string());
            args.push("aac".to_string());
        }
        Container::WebM => {
            args.push("-c:a".to_string());
            args.push("libopus".to_string());
        }
    }
    args.push("-b:a".to_string());
    args.push(bitrate.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ExportPreset {
        ExportPreset::default()
    }

    #[test]
    fn minimal_mp4_balanced_everyday_case() {
        let args = build_ffmpeg_args(&base(), "input.mp4", "output.mp4", None);
        assert_eq!(
            args,
            vec![
                "-hide_banner",
                "-i",
                "input.mp4",
                "-c:v",
                "libx264",
                "-crf",
                "23",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                "-c:a",
                "aac",
                "-b:a",
                "96000",
                "output.mp4",
            ]
        );
    }

    #[test]
    fn width_cap_and_fps_and_high_quality() {
        let preset = ExportPreset {
            max_width: 1080,
            fps: 30,
            quality: Quality::High,
            audio: Audio::Aac128,
            ..base()
        };
        let args = build_ffmpeg_args(&preset, "in.mov", "out.mp4", None);
        let vf = args.iter().position(|a| a == "-vf").unwrap();
        assert_eq!(args[vf + 1], "scale='min(iw,1080)':-2");
        let r = args.iter().position(|a| a == "-r").unwrap();
        assert_eq!(args[r + 1], "30");
        let crf = args.iter().position(|a| a == "-crf").unwrap();
        assert_eq!(args[crf + 1], "18");
    }

    #[test]
    fn webm_uses_vp9_and_opus() {
        let preset = ExportPreset {
            container: Container::WebM,
            ..base()
        };
        let args = build_ffmpeg_args(&preset, "in.mp4", "out.webm", None);
        let cv = args.iter().position(|a| a == "-c:v").unwrap();
        assert_eq!(args[cv + 1], "libvpx-vp9");
        let bv = args.iter().position(|a| a == "-b:v").unwrap();
        assert_eq!(args[bv + 1], "0");
        let ca = args.iter().position(|a| a == "-c:a").unwrap();
        assert_eq!(args[ca + 1], "libopus");
        assert!(!args.contains(&"-movflags".to_string()));
    }

    #[test]
    fn audio_none_strips_audio() {
        let preset = ExportPreset {
            audio: Audio::None,
            ..base()
        };
        let args = build_ffmpeg_args(&preset, "in.mp4", "out.mp4", None);
        assert!(args.contains(&"-an".to_string()));
        assert!(!args.contains(&"-c:a".to_string()));
    }

    #[test]
    fn watermark_chain_bottom_right_with_scaled_main() {
        let preset = ExportPreset {
            max_width: 1280,
            watermark: Some(Watermark {
                width_fraction: 0.15,
                corner: Corner::BottomRight,
                margin_px: 16,
            }),
            ..base()
        };
        // 1920 source capped to 1280 → watermark = 15% of 1280 = 192.
        assert_eq!(resolve_watermark_width(&preset, 1920), 192);
        let args = build_ffmpeg_args(&preset, "in.mp4", "out.mp4", Some(192));
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        assert_eq!(
            args[fc + 1],
            "[0:v]scale='min(iw,1280)':-2[v0];[1:v]scale=192:-1[wm];\
             [v0][wm]overlay=main_w-overlay_w-16:main_h-overlay_h-16[vout]"
        );
    }

    #[test]
    fn watermark_chain_without_main_scale_top_left() {
        let preset = ExportPreset {
            watermark: Some(Watermark {
                width_fraction: 0.2,
                corner: Corner::TopLeft,
                margin_px: 8,
            }),
            ..base()
        };
        let args = build_ffmpeg_args(&preset, "in.mp4", "out.mp4", Some(128));
        let fc = args.iter().position(|a| a == "-filter_complex").unwrap();
        assert_eq!(
            args[fc + 1],
            "[0:v]null[v0];[1:v]scale=128:-1[wm];[v0][wm]overlay=8:8[vout]"
        );
        // filter_complex mode maps video explicitly, and keeps optional audio.
        assert!(args.contains(&"[vout]".to_string()));
        assert!(args.contains(&"0:a?".to_string()));
        // The second input (the PNG) must come after the first input.
        let first_i = args.iter().position(|a| a == "-i").unwrap();
        let second_i = args.iter().rposition(|a| a == "-i").unwrap();
        assert!(second_i > first_i);
        assert_eq!(args[second_i + 1], WATERMARK_FILENAME);
    }

    #[test]
    fn watermark_ignored_without_width() {
        // Defensive: a preset with a watermark but no resolved width falls
        // back to the plain path rather than producing a broken command.
        let preset = ExportPreset {
            watermark: Some(Watermark {
                width_fraction: 0.2,
                corner: Corner::TopLeft,
                margin_px: 8,
            }),
            ..base()
        };
        let args = build_ffmpeg_args(&preset, "in.mp4", "out.mp4", None);
        assert!(!args.contains(&"-filter_complex".to_string()));
        assert!(!args.contains(&WATERMARK_FILENAME.to_string()));
    }

    #[test]
    fn resolve_watermark_width_caps_and_floors() {
        let preset = ExportPreset {
            max_width: 1080,
            watermark: Some(Watermark {
                width_fraction: 0.15,
                corner: Corner::TopLeft,
                margin_px: 8,
            }),
            ..base()
        };
        // 4K source capped to 1080 → 15% of 1080.
        assert_eq!(resolve_watermark_width(&preset, 3840), 162);
        // Tiny source stays at the 8px floor.
        assert_eq!(resolve_watermark_width(&preset, 40), 8);
    }

    #[test]
    fn output_always_last_arg() {
        for preset in [
            base(),
            ExportPreset {
                container: Container::WebM,
                audio: Audio::None,
                watermark: Some(Watermark {
                    width_fraction: 0.1,
                    corner: Corner::TopRight,
                    margin_px: 4,
                }),
                ..base()
            },
        ] {
            let args = build_ffmpeg_args(&preset, "in", "out", Some(120));
            assert_eq!(args.last().unwrap(), "out");
        }
    }
}
