# Vendored transcoding engine

These files are the engine this app drives for all video work. They are
vendored (committed) rather than npm-installed so the build stays a pure
`cargo build` + `wasm-bindgen` + copy, with no `node_modules`, and the desktop
Pro build embeds them into `dist/` offline.

| File                 | Source                          | What it is                                                                 |
| -------------------- | ------------------------------- | -------------------------------------------------------------------------- |
| `mediabunny.min.mjs` | `mediabunny` (MPL-2.0), npm, vendored 2026-10-10 | Demux, WebCodecs decode/encode and WebM mux library (ES module, minified) |
| `mediabunny.LICENSE` | `mediabunny` (MPL-2.0)          | Its licence, shipped next to the minified build                             |
| `engine.js`          | this repo                       | Thin glue: `capabilities()`, `probe()`, `compress()`, `cancel()` for `src/engine.rs` |

All three ship **flat, next to the app's wasm-bindgen glue**, in every hosting
context (hub `public/apps/video-compressor/`, trunk `dist/`, desktop embedded
`dist/`). `src/engine.rs` loads `engine.js` relative to the glue's own URL, and
`engine.js` imports `mediabunny.min.mjs` relative to itself.

Version bump procedure: `npm pack mediabunny@<v>`, copy the minified ESM build
and `LICENSE` from the package, update the table above, then re-run the
browser validation in `todo.md` (Phase 5).

## Royalty-free policy

No codec implementation ships with this app.

- **Decode** (H.264, HEVC, AAC, and whatever else the input contains) is done by
  the browser's or OS's own WebCodecs decoders. Those decoders are licensed by
  their vendors, not by this app.
- **Encode** is AV1, VP9 or VP8 video plus Opus audio, written to WebM. These
  are royalty-free and come from the browser's own WebCodecs encoders.

So the app never distributes H.264/AAC/x264/FFmpeg code. Whether relying on
OS and browser codecs fully removes patent exposure still needs a legal opinion
(`todo.md`, Phase 5).

## Requirements

A current Chromium browser (Chrome or Edge) with WebCodecs. The desktop Pro
shell runs on WebView2, which is also Chromium. Other browsers are untested.
`engine.js`'s `capabilities()` checks for WebCodecs and an AV1/VP9/VP8 encoder
up front, so an unsupported browser gets a clear message rather than a failure
mid-encode.
