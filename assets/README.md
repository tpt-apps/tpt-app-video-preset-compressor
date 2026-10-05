# Vendored ffmpeg.wasm runtime

These files are the ffmpeg.wasm engine this app drives for all video work.
They are vendored (committed) rather than npm-installed so the build pipeline
stays a pure `cargo build` + `wasm-bindgen` + copy — no node_modules, and the
desktop Pro build embeds them into `dist/` offline.

| File             | Source package           | Version | What it is                                              |
| ---------------- | ------------------------ | ------- | ------------------------------------------------------- |
| `ffmpeg.js`      | `@ffmpeg/ffmpeg` (MIT)   | 0.12.15 | UMD wrapper (`dist/umd/ffmpeg.js`), exposes `FFmpegWASM` |
| `814.ffmpeg.js`  | `@ffmpeg/ffmpeg` (MIT)   | 0.12.15 | its web-worker chunk (`dist/umd/814.ffmpeg.js`), passed to `load()` as `classWorkerURL` |
| `ffmpeg-core.js` | `@ffmpeg/core` (GPL-2.0-or-later) | 0.12.10 | ESM core glue (`dist/esm/ffmpeg-core.js`) |
| `ffmpeg-core.wasm` | `@ffmpeg/core` (GPL-2.0-or-later) | 0.12.10 | the ffmpeg engine itself (single-threaded build — the hub does not set COOP/COEP, so SharedArrayBuffer/multithread is unavailable) |

All four are shipped **flat, next to the app's wasm-bindgen glue**, in every
hosting context (hub `public/apps/video-compressor/`, trunk `dist/`, desktop
embedded `dist/`); `src/ffmpeg.rs` resolves them relative to the glue's own
URL at runtime.

Version bump procedure: `npm pack @ffmpeg/ffmpeg@<v> @ffmpeg/core@<v>`, copy
the four files from `dist/umd` / `dist/esm` as above, update this table and
the licenses note below, re-run the browser validation in `todo.md`.

## Licensing

`@ffmpeg/ffmpeg` (the wrapper) is MIT. `@ffmpeg/core` is **GPL-2.0-or-later**
(it links x264). Distributing anything that includes the core (the hub bundle,
the Gumroad exe) carries GPL source obligations for the core itself — the
corresponding source is the upstream ffmpeg.wasm repo at the versions above.
This matches the app spec (apps.md §T2-15: "base: ffmpeg-wasm").
