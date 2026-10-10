// TPT Video Compressor engine: royalty-free transcoding on the browser's own
// WebCodecs, via mediabunny (MPL-2.0, vendored next to this file).
//
// No codec code ships with the app. Decoding H.264/HEVC/AAC is done by the
// browser / OS decoders (licensed by their vendors); output is royalty-free
// (AV1 / VP9 / VP8 video + Opus audio in WebM).
//
// Loaded from Rust with a dynamic import (src/engine.rs); every function
// takes and returns plain JS values so the Rust side stays thin.

import * as mb from './mediabunny.min.mjs';

const CODEC_CHAINS = {
  av1: ['av1', 'vp9', 'vp8'],
  vp9: ['vp9', 'av1', 'vp8'],
  vp8: ['vp8', 'vp9', 'av1'],
};

// Quality is a video bitrate budget: target bits per pixel per frame. The
// presets are deliberately not mediabunny's QUALITY_* constants, which are far
// higher (about 5 Mbps at 1080p for one test clip, so a "compressed" file came
// out bigger than its 3 Mbps H.264 source).
const BITS_PER_PIXEL = { high: 0.10, balanced: 0.07, small: 0.04 };
const DEFAULT_FPS = 30;

function videoBitrate(quality, width, height, fps) {
  const bpp = BITS_PER_PIXEL[quality] ?? BITS_PER_PIXEL.balanced;
  return Math.max(200_000, Math.round(width * height * fps * bpp));
}

/** The source's average frame rate, or 30 when it can't be measured. */
async function sourceFrameRate(track) {
  try {
    const rate = (await track.computePacketStats(120)).averagePacketRate;
    if (rate > 0) return rate;
  } catch (_) { /* fall through to the default */ }
  return DEFAULT_FPS;
}

function openInput(file) {
  return new mb.Input({ source: new mb.BlobSource(file), formats: mb.ALL_FORMATS });
}

/** True when this browser can decode + encode what the app needs. */
export async function capabilities() {
  const hasWebCodecs = typeof VideoEncoder !== 'undefined' && typeof VideoDecoder !== 'undefined';
  if (!hasWebCodecs) return { supported: false, reason: 'This browser has no WebCodecs support. Use a current Chrome or Edge.' };
  const video = await mb.getFirstEncodableVideoCodec(['av1', 'vp9', 'vp8'], { width: 1280, height: 720 });
  if (!video) return { supported: false, reason: 'This browser cannot encode AV1, VP9 or VP8 video.' };
  const opus = await mb.canEncodeAudio('opus');
  return { supported: true, opus };
}

/** Reads container metadata. Throws if the file is not a readable video. */
export async function probe(file) {
  const input = openInput(file);
  try {
    const video = await input.getPrimaryVideoTrack();
    if (!video) throw new Error('no video stream found (is this really a video file?)');
    const audio = await input.getPrimaryAudioTrack();
    const duration = await input.computeDuration();
    let fps = 0;
    try {
      fps = (await video.computePacketStats(120)).averagePacketRate || 0;
    } catch (_) { /* fps stays 0 = unknown */ }
    return {
      duration_secs: duration || 0,
      width: video.displayWidth,
      height: video.displayHeight,
      fps,
      video_codec: video.codec || 'unknown',
      can_decode_video: await video.canDecode(),
      audio: audio
        ? {
            codec: audio.codec || 'unknown',
            sample_rate_hz: audio.sampleRate,
            channels: audio.numberOfChannels,
            can_decode: await audio.canDecode(),
          }
        : null,
    };
  } finally {
    input.dispose?.();
  }
}

let current = null;

/** Cancels the running compress(), if any. */
export async function cancel() {
  if (current) await current.cancel();
}

/**
 * Compresses `file` to WebM.
 * spec: { codec: 'av1'|'vp9'|'vp8', quality: 'high'|'balanced'|'small',
 *         max_width: number (0 = original), fps: number (0 = original),
 *         audio_bitrate: number (0 = strip audio),
 *         watermark: null | { fraction, corner: 'tl'|'tr'|'bl'|'br', margin } }
 * watermarkBytes: PNG bytes (Uint8Array) when spec.watermark is set.
 * onProgress(ratio 0..1).
 * fileHandle (optional FileSystemFileHandle, from the save picker): when given
 * the WebM is streamed straight into that file in chunks, so a long output
 * never has to fit in memory. Otherwise it is buffered in memory.
 * Resolves { blob, codec }.
 */
export async function compress(file, spec, watermarkBytes, onProgress, fileHandle) {
  const input = openInput(file);
  const videoTrack = await input.getPrimaryVideoTrack();
  if (!videoTrack) throw new Error('no video stream found (is this really a video file?)');
  if (!(await videoTrack.canDecode())) {
    throw new Error(
      `this browser cannot decode the video (${videoTrack.codec || 'unknown codec'}). ` +
      'Try a current Chrome or Edge, or a different file.'
    );
  }

  // Never upscale; keep both dimensions even.
  const srcW = videoTrack.displayWidth, srcH = videoTrack.displayHeight;
  const cap = spec.max_width > 0 ? Math.min(srcW, spec.max_width) : srcW;
  const outW = Math.max(2, Math.round(cap / 2) * 2);
  const outH = Math.max(2, Math.round((srcH * outW) / srcW / 2) * 2);

  const frameRate = spec.fps > 0 ? spec.fps : await sourceFrameRate(videoTrack);
  const bitrate = videoBitrate(spec.quality, outW, outH, frameRate);
  const codec = await mb.getFirstEncodableVideoCodec(CODEC_CHAINS[spec.codec] || CODEC_CHAINS.vp9, {
    width: outW, height: outH, bitrate,
  });
  if (!codec) throw new Error('this browser cannot encode AV1, VP9 or VP8 video.');

  const video = { codec, bitrate, width: outW, height: outH, fit: 'fill' };
  if (spec.fps > 0) video.frameRate = spec.fps;

  if (spec.watermark && watermarkBytes) {
    const bitmap = await createImageBitmap(new Blob([watermarkBytes], { type: 'image/png' }));
    const wm = spec.watermark;
    const wmW = Math.max(8, Math.round(outW * wm.fraction));
    const wmH = Math.max(1, Math.round((bitmap.height * wmW) / bitmap.width));
    const x = wm.corner === 'tl' || wm.corner === 'bl' ? wm.margin : outW - wmW - wm.margin;
    const y = wm.corner === 'tl' || wm.corner === 'tr' ? wm.margin : outH - wmH - wm.margin;
    const canvas = new OffscreenCanvas(outW, outH);
    const ctx = canvas.getContext('2d');
    video.processedWidth = outW;
    video.processedHeight = outH;
    video.process = (sample) => {
      sample.draw(ctx, 0, 0, outW, outH);
      ctx.drawImage(bitmap, x, y, wmW, wmH);
      return canvas;
    };
  }

  const audio = spec.audio_bitrate > 0
    ? { codec: 'opus', bitrate: spec.audio_bitrate }
    : { discard: true };

  let writable = null;
  let target;
  if (fileHandle) {
    writable = await fileHandle.createWritable();
    target = new mb.StreamTarget(writable, { chunked: true, chunkSize: 16 * 1024 * 1024 });
  } else {
    target = new mb.BufferTarget();
  }
  const output = new mb.Output({ format: new mb.WebMOutputFormat(), target });
  const conversion = await mb.Conversion.init({ input, output, video, audio });
  if (!conversion.isValid) {
    await writable?.abort().catch(() => {});
    throw new Error(
      'nothing could be converted: ' + conversion.discardedTracks.map((d) => d.reason).join(', ')
    );
  }
  if (onProgress) conversion.onProgress = (p) => onProgress(p);
  current = conversion;
  try {
    await conversion.execute();
  } catch (err) {
    await writable?.abort().catch(() => {});
    throw err;
  } finally {
    current = null;
    input.dispose?.();
  }
  if (fileHandle) {
    // The streamed file is complete; hand back the file as the result.
    return { blob: await fileHandle.getFile(), codec, streamed: true };
  }
  return {
    blob: new Blob([output.target.buffer], { type: 'video/webm' }),
    codec,
  };
}
