# TPT Video Compressor — Build Todo

> Hub listing: `tptsolutions.co.nz/tools/video-compressor` (registry slug
> `video-compressor`, category `utility`). Spec: `apps.md` §T2-15 in the
> tpt-electrician-nz-2 repo. Base is ffmpeg-wasm. Free version: one clip, no
> saved presets. Paid offline edition ($19 USD, Gumroad): unlimited saved
> presets, batch folder processing, no length limits. UI on `tpt-appfront`.
>
> Hub contract: `wasm-bindgen --target web` glue exposes `mount(container)`
> (called by `WasmAppRunner` after `default()` init). Standalone `trunk serve`
> mounts into the `#tpt-appfront-root` marker div instead — never
> `document.body` — so the two hosts never double-mount.
>
> STATUS: built and browser-verified end to end (free + pro + desktop exe).
> Remaining = the manual owner steps at the bottom.

## Phase 0 — Scaffold & pipeline proof ✅
- [x] Scaffold by hand mirroring `tpt-app-fea-lite` (the `tpt-appfront init`
      template exists but fea-lite's hub-contract layout is the real pattern)
- [x] Hub contract: `#[wasm_bindgen] pub fn mount(el)` + `#[wasm_bindgen(start)]`
      no-op unless `#tpt-appfront-root` present
- [x] `index.html` carries the marker div (+ copy-file hooks for the ffmpeg assets)
- [x] Build pipeline `build.ps1`/`build.sh` mirroring fea-lite's
      (`--target web` glue → web repo `public/apps/video-compressor/`)
- [x] Hello-world build loads through `trunk serve` (browser-verified)
- [x] git init + commits
- [x] ffmpeg-wasm integration resolved: drive the **vendored @ffmpeg/ffmpeg
      UMD + @ffmpeg/core** from Rust via `js_sys`/`web_sys` (see
      `src/ffmpeg.rs`). Critical finding: do NOT pass `classWorkerURL` — in
      0.12.15 that spawns a module worker whose dynamic-import fallback is a
      webpack stub that always throws. Without it the wrapper spawns its own
      classic worker, which requires the UMD core build (importScripts).

## Phase 1 — Core engine (wrap ffmpeg-wasm) ✅
- [x] Engine in a worker (never blocks the UI thread): UMD wrapper injected
      via `<script>`, worker chunk resolved from the script tag's own URL
- [x] Adapter layer: `tpt-video-engine` (pure, host-tested) —
      `ExportPreset` model → ffmpeg argv (`cmd::build_ffmpeg_args`) and
      `ffmpeg -i` stderr → `MediaInfo` (`probe::parse_probe_log`)
- [x] Preset model: container, max width, fps, CRF quality, audio bitrate,
      optional PNG watermark (corner/margin/width %). Pro persists presets
      to localStorage as JSON
- [x] Validation: sample clip probes correctly (640×360 h264/aac parsed from
      the worker's stderr) and compresses under several presets; output
      verified (ftyp/moov-faststart, playable in <video>)
- [x] **VP9/WebM dropped from the UI**: the 0.12.10 core's libvpx-vp9 traps
      its wasm memory mid-encode ("memory access out of bounds"). The engine
      keeps the code path for a future core bump.

## Phase 2 — Free tier ✅
- [x] Single-clip upload (file picker + drag & drop), probe, compress,
      preview, download
- [x] Free-tier limits chosen to make the paid bullets true: 200 MiB input,
      5-minute duration → `LimitHit` upsell message
- [x] Free-tier upsell card → Pro edition
- [x] Browser-verified end to end (dev page + the hub page's WasmAppRunner):
      mount, upload, probe metadata line, 45% smaller MP4, playable result,
      download button

## Phase 3 — Paid desktop edition (cargo feature `pro`) ✅
- [x] `pro` gates: saved presets manager (create/apply/delete, localStorage),
      batch queue (multi-file + webkitdirectory folder pick, sequential run,
      per-row status + downloads), PNG branding overlay, no length/size caps
- [x] Pro UI browser-verified on the `--features pro` trunk bundle: Pro chip,
      overlay fields, preset save + persistence round-trip, batch of 2 clips
      → done rows with downloads, no free upsell card
- [x] Desktop shell via `tpt-appfront-webview`; `build.ps1 -Pro` produces
      `target\release\tpt-video-compressor-pro.exe` (embeds `dist\`, extracts
      to temp on first run — the customer double-click path, smoke-tested
      from a foreign CWD). ffmpeg.wasm kept as the engine (no native ffmpeg
      sidecar); revisit only if long-clip performance becomes a complaint
- [x] `release\TPT-Video-Compressor-Pro.zip` packaged for the Gumroad upload;
      `gumroad-cover.png` (real Pro screenshot with client presets)

## Phase 4 — Ship & measure ✅ (manual steps remain)
- [x] Free bundle copied to the web repo `public/apps/video-compressor/`
      (glue + wasm + 4 ffmpeg files, flat), built with `--features hub` so
      asset URLs are site-absolute
- [x] Registry entry added (`coming-soon`, `kind: 'demo'`, `price: '$19'`,
      proFeatures from §T2-15); verified in `npm run dev`: card on /tools,
      detail page, sitemap entry, WasmAppRunner mount, in-page compress
- [x] WASM size budget: **app payload 104 KB gz** (budget ~2 MB) —
      ffmpeg-core.wasm is 9.9 MB gz and is vendored upstream code, tracked
      separately (see assets/README.md)
- [x] apps.md checklist row: Built ✅

## Remaining (manual, owner)
1. Gumroad: create the product from `GUMROAD.md` copy, upload
   `release\TPT-Video-Compressor-Pro.zip`, cover `gumroad-cover.png`.
2. Paste the product URL into the registry entry's `gumroadUrl`, flip
   `status` to `'live'`, commit + push + deploy the web repo.
3. Optional: bump `@ffmpeg/core` (>= 0.12.10) to re-test VP9/WebM —
   see assets/README.md bump procedure.
