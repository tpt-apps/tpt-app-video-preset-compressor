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

## Phase 0 — Scaffold & pipeline proof
- [ ] `tpt-appfront init tpt-app-video-preset-compressor --target dom` (path deps on the tpt-appfront checkout)
- [ ] Restructure scaffold to hub contract: `#[wasm_bindgen] pub fn mount(el)` + `#[wasm_bindgen(start)]` no-op unless `#tpt-appfront-root` present
- [ ] `index.html` carries the `#tpt-appfront-root` marker div
- [ ] Build pipeline script mirroring fea-lite/cutlist-optimizer's (`--target web` glue → web repo `public/apps/video-compressor/`)
- [ ] Hello-world build loads through the standalone dev page (`trunk serve`)
- [ ] git init + first commit
- [ ] Note: base is ffmpeg-wasm (a JS/wasm build of ffmpeg), not a `tpt-*`
      Rust crate — confirm how this integrates with the `tpt-appfront` DOM
      backend (likely a JS interop boundary rather than a pure-Rust adapter)

## Phase 1 — Core engine (wrap ffmpeg-wasm)
- [ ] Integrate ffmpeg-wasm into the `tpt-appfront` build (JS interop from
      the wasm-bindgen glue, or a thin JS wrapper the Rust UI calls into)
- [ ] Adapter layer: video file + export preset (codec, resolution, bitrate,
      container) in → compressed video file out
- [ ] Preset model: define what a "preset" captures (per-client export
      settings) and how it's stored/loaded
- [ ] Validation: a sample clip compresses correctly and remains playable
      across a couple of common export presets

## Phase 2 — Free tier
- [ ] Single-clip upload + one export preset, no saved presets, download result
- [ ] Free-tier upsell copy pointing at unlimited saved presets, batch folder
      processing, no length limits
- [ ] Browser-verified: mount, upload, compress, result plays correctly

## Phase 3 — Paid desktop edition (cargo feature `pro`)
- [ ] `pro` gates: unlimited saved per-client presets, batch folder
      processing, no file-length limits
- [ ] Pro UI: preset manager (create/save/load per client), folder-batch
      queue with progress
- [ ] Desktop shell via `tpt-appfront-webview`; Windows exe build + smoke test
      (confirm ffmpeg-wasm performance is acceptable for longer clips
      desktop-side, or swap to a native ffmpeg binary for the pro edition)
- [ ] Gumroad product from apps.md listing copy ($19), exe upload, URL into
      registry — manual step, listing copy at apps.md §T2-15

## Phase 4 — Ship & measure
- [ ] Registry entry (`utility` category, `wasm: { entry: '/apps/video-compressor/video-compressor.js' }`)
      per the "Adding a finished app to the hub" runbook at the bottom of apps.md
- [ ] Verify hub card/detail/sitemap/runner in dev, then prod after deploy
- [ ] WASM size budget check on the shipped build (ffmpeg-wasm itself is
      large — track its size separately from app code)
