# Gumroad listing

Everything needed to create/update the Gumroad product once this app is
built. Source copy is `apps.md` §T2-15 in the `tpt-electrician-nz-2` repo — if
that changes, update here too (and vice versa).

## Step-by-step: publish this listing

1. Build the Pro edition (`./build.ps1 -Pro`, once the build pipeline exists
   — see `todo.md` and mirror `tpt-app-fea-lite`/`tpt-app-cutlist-optimizer`'s
   pattern) — produces the packaged exe/zip in `release/`.
2. Log in to Gumroad → **New product** → **Digital product**.
3. Set the product name, price, and upload the packaged file (all below).
4. Paste the short description, bullets, and full description (below) into
   their respective fields.
5. Set category/tags (below) and add a cover image (see note below).
6. Publish, then copy the resulting product URL (`https://gum.co/...`).
7. Paste that URL into the `video-compressor` entry's `gumroadUrl` field in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$19'`
   on the same entry.
8. Flip that entry's `status` to `'live'` once both the free demo and this
   Gumroad listing exist.
9. Commit and push the registry repo.

## File to upload

`release\TPT-Video-Compressor-Pro.zip` (built by `./build.ps1 -Pro`; the zip
holds the single self-contained exe — it embeds its web bundle and extracts
it to temp on first run). Cover image: `gumroad-cover.png` in this repo (a
real screenshot of the Pro UI with client presets saved). Rebuild both after
any source change: `./build.ps1 -Pro`, then re-zip if the exe changed.

## Product name

```
TPT Video Compressor — Saved Client Presets for Batch Video Compression
```

## Price

**$19 USD, one-time purchase** (no subscription).

## Short description / summary field

```
Stop re-configuring export settings for every client. Save presets per client and batch-compress in one click.
```

## Bullets / feature list

```
Saved per-client presets
Batch folder processing
No file-size/length limits
Works offline
One-time purchase
```

## Full description (long-form field)

```
TPT Video Compressor compresses/converts video with saved, reusable presets
per client (target platform, resolution, bitrate, branding overlay), so
repeat jobs for the same client are one click instead of re-configuring
settings every time.

Who it's for: social media managers and small marketing agencies handling
video assets across multiple clients, each with different platform/brand
size and format requirements. HandBrake is free and excellent at raw
compression but has no concept of "client presets" — every job starts from
scratch.

Pro edition (this download):
- Unlimited saved per-client presets (free has none saved)
- Batch folder processing
- Longer file support with no browser memory limits

Try the free single-clip edition first, right in your browser, no install:
tptsolutions.co.nz/tools/video-compressor

Requirements: Windows 10 (20H2+) or Windows 11. Uses the WebView2 runtime,
preinstalled on both — nothing extra to download.
```

## Category / tags

- Category: **Software > Utility** (or Gumroad's closest equivalent)
- Tags: `video-compression`, `social-media-tools`, `marketing-agency`,
  `batch-processing`, `content-creation`, `windows`

## Cover image / thumbnail

Not created yet. A screenshot of a saved preset list with several clients
named is the most convincing cover once built. No stock/AI art.

## After publishing

1. Copy the resulting product URL (`https://gum.co/...`).
2. Paste it into the `gumroadUrl` field of the `video-compressor` entry in
   `tpt-electrician-nz-2\src\lib\tpt-apps-registry.ts`, and set `price: '$19'`.
3. Set `status` to `'live'`.
4. Commit and push the registry repo.
