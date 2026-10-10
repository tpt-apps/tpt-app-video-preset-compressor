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
- [x] Preset model: container, max width, fps, quality level, audio bitrate,
      optional PNG watermark (corner/margin/width %). Pro persists presets
      to localStorage as JSON
- [x] Validation: sample clip probes correctly (640×360 h264/aac parsed from
      the worker's stderr) and compresses under several presets; output
      verified (ftyp/moov-faststart, playable in <video>)
- [x] **Royalty-free output only (policy)**: H.264/AAC are patent-encumbered
      and this app ships no codec licences, so output is **WebM = VP8 + Opus**
      only (`engine/src/cmd.rs`; a test asserts no preset can emit libx264/aac).
      Old saved presets (`"mp4"`/`"aac*"`) load as WebM/Opus via serde aliases.
      The 0.12.10 core's libvpx-vp9 traps its wasm memory mid-encode on every
      flag combination (re-verified headless in node), and the core has no AV1
      encoder, so VP8 is the best royalty-free codec available today.

## Phase 2 — Free tier ✅
- [x] Single-clip upload (file picker + drag & drop), probe, compress,
      preview, download
- [x] Free-tier limits chosen to make the paid bullets true: 200 MiB input,
      5-minute duration → `LimitHit` upsell message
- [x] Free-tier upsell card → Pro edition
- [x] Browser-verified end to end (dev page + the hub page's WasmAppRunner):
      mount, upload, probe metadata line, 45% smaller MP4 (pre-policy; now WebM), playable result,
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

## Phase 5 — Royalty-free WebCodecs engine (replaces ffmpeg.wasm) ✅ (code + browser-verified; exe encode open)
Policy: ship **no patent-encumbered codec code**. Decode (H.264/HEVC/AAC) is
done by the browser/OS's own licensed WebCodecs decoders; output is
royalty-free (AV1 / VP9 / VP8 + Opus in WebM). Spike (Chrome + Edge, 618 MB
H.264/AAC source, VP8/VP9/AV1 all OK) passed 2026-10-10.
- [x] Spike: mediabunny (MPL-2.0) demux → WebCodecs decode → encode → WebM mux
- [x] Vendor `mediabunny.min.mjs` + its licence into `assets/`; `assets/engine.js`
      (probe / compress / codec fallback / watermark)
- [x] Engine crate: `EncodeSpec` (codec, width cap, fps, quality, Opus bitrate,
      watermark) and serde `MediaInfo`; `VideoCodec` (AV1/VP9/VP8) on presets
- [x] `src/ffmpeg.rs` replaced by `src/engine.rs` (JS interop); ffmpeg worker,
      log and progress plumbing dropped from `src/app.rs`
- [x] UI: video codec select (Auto/VP9, Smallest/AV1, Compatible/VP8);
      unsupported-input and no-encoder messages from `probe` / `capabilities`
- [x] Batch: keeps `File` handles instead of copying every file into wasm memory
- [x] Removed `assets/ffmpeg*.js|wasm` (32 MB core) and updated `index.html`,
      `build.ps1`/`build.sh`, `justfile`, `assets/README.md`, and `GUMROAD.md`
      copy (removes the GPL core obligation)
- [x] Browser-verified (headless Chrome, `trunk` dev + Pro bundles): probe;
      compress VP9 / AV1 / VP8 with Small / Balanced / High; output is WebM,
      plays back at the right size; a non-video file shows an error; Pro
      watermark overlay; Pro two-file batch. **Not yet exercised:** the hub
      page's `WasmAppRunner` mount (web repo not run here).
- [x] Fixed on verification: `dyn_into::<Object>` rejected the module namespace;
      mediabunny's `QUALITY_*` presets produced files larger than the source,
      so quality is now a bits-per-pixel bitrate budget (`assets/engine.js`)
- [x] Verified inside the real WebView2 exe (driven over WebView2's DevTools
      port): VP9, AV1 and VP8 at Small/Balanced/High all encode and play. 8 s
      720p clip: 1.0 s (VP9) to 1.6 s (AV1). The exe runs at `https://app.localhost`,
      a secure context where the native `showSaveFilePicker` is present, so no
      custom save-dialog bridge is needed. VP9 stays the default.
- [x] Streamed save verified inside the exe: the picker is called once and the
      output streams to the chosen file (2.2 MB for a 30 s movie segment).
- [x] Hub page (`/tools/video-compressor` in the web repo, dev server): the
      runner mounts the app, compresses, plays back, no console errors.
- [x] Quality levels re-tuned on real footage (SSIM vs source, headless
      Chrome + ffmpeg `ssim`). Output is now a fraction of the **source's own
      bitrate** (High 1.0, Balanced 0.7, Small 0.4, floor 0.01 bits/pixel/frame,
      scaled for downscaling). Measured on a 30 s movie segment (source 0.9 Mbps
      video): Small 0.40x size / SSIM 0.955, Balanced 0.61x / 0.961, High 0.83x /
      0.963. On a grainy 5 Mbps 6 s vertical clip: 0.37x / 0.892, 0.62x / 0.908,
      0.86x / 0.916. The old fixed pixel budget grew the movie to 1.5-2.1x its size.
- [x] Decide free-tier limits: **kept at 200 MiB / 5 min**. The memory reason
      is gone (input is read lazily), but they remain the clear Pro upsell.
      Revisit only if sales data says otherwise.
- [x] Large outputs: Pro single-clip output streams into a file chosen with
      the save picker (`showSaveFilePicker`, `StreamTarget`, chunked). Verified
      headless: streamed bytes match the buffered encode, playable, cancel is
      clean. Falls back to in-memory `BufferTarget` where the picker is missing
      (the desktop `app://` shell may lack it: unverified).
- [x] Batch output streams into a folder chosen with `showDirectoryPicker`
      (one file per clip, same name as the single-clip output; an existing file
      with that name is replaced). Verified headless and inside the exe: both
      clips land in the folder at the same sizes as single-clip runs. Cancelling
      the folder dialog leaves the queue pending. Falls back to memory without
      the picker.
- [x] Rebuilt Pro exe (`release/TPT-Video-Compressor-Pro.zip`, exe 2.0 MB),
      free bundle in the web repo's `public/apps/video-compressor/` (stale
      ffmpeg files removed), and retook `gumroad-cover.png` from the Pro UI
- [ ] Legal: get a professional opinion that relying on OS/browser codecs
      removes the patent-fee exposure for distributing this app

## Remaining (manual, owner)
1. Review the uncommitted web-repo changes in `public/apps/video-compressor/`
   and the Gumroad assets, then commit.
2. Gumroad: create the product from `GUMROAD.md` copy, upload
   `release\TPT-Video-Compressor-Pro.zip`, cover `gumroad-cover.png`.
3. Paste the product URL into the registry entry's `gumroadUrl`, flip
   `status` to `'live'`, commit + push + deploy the web repo.
4. Decide the free-tier limits and the legal opinion (Phase 5 above).

## Licensing follow-ups (royalty-free policy)
- **Resolved:** the ffmpeg core (GPL, built `--enable-gpl` with x264/x265) is
  no longer shipped anywhere in this app.
- **Input decoding**: reading users' H.264/HEVC/AAC files runs the browser's
  and OS's licensed decoders. That is residual decoder-patent exposure that
  the output-only policy does not remove. Needs the legal opinion above.
- Alternative engine (tpt-kinetix) is still not ready and is not needed now.
