# TPT Video Compressor - build pipeline (Windows).
#
# Free edition (default):  ./build.ps1
#   cargo build --release --target wasm32-unknown-unknown (no pro feature)
#   wasm-bindgen --target web  ->  web repo public/apps/video-compressor/
#   when TPT_WEB_REPO is set (glue named video-compressor.js, wasm keeps its
#   _bg.wasm name - the glue resolves it via import.meta.url), otherwise
#   dist\web. The vendored engine (assets/engine.js + mediabunny) is copied
#   flat next to the glue either way - src/engine.rs resolves engine.js
#   relative to the glue module's own URL at runtime.
# Pro desktop edition:     ./build.ps1 -Pro
#   trunk build --release --features pro (its own independent wasm build,
#   into dist\ - NOT the wasm-bindgen path above; trunk also copies the
#   engine assets into dist\ via index.html's copy-file hooks) + cargo
#   build the native webview exe, which embeds dist\ into the exe.
#   Deliberately does not touch public/apps/video-compressor/: that is the
#   free in-browser demo's bundle, and it must never be built with
#   --features pro, or the site would give away every paid feature (saved
#   presets, batch folders, no length limits) for free. The two editions
#   are separate cargo builds writing to separate directories on purpose -
#   do not try to "reuse" one build for both.
#
# Env:
#   TPT_WEB_REPO  path to the tptsolutions.co.nz web repo; the app is copied
#                 to <repo>\public\apps\video-compressor\ for the hub runner.
#                 Defaults to the known local checkout below if unset.

[CmdletBinding()]
param(
    # Build the Pro desktop bundle (--features pro) instead of the free one.
    # Never writes to the free edition's $Dest - see header comment.
    [switch]$Pro,
    # Free-edition only: skip the cargo build and re-run wasm-bindgen + asset
    # copy against an existing release build.
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-Location -LiteralPath $PSScriptRoot

$Crate = "tpt-app-video-preset-compressor"
$Target = "wasm32-unknown-unknown"
$OutName = "video-compressor"
$Slug = "video-compressor"
$DefaultWebRepo = "D:\Programming\2 WIP\TPT Electrician\tpt-electrician-nz-2"

# The vendored transcoding engine, shipped flat next to the glue. Decoding
# and encoding run on the browser's own WebCodecs; no codec code is shipped.
$EngineAssets = @("engine.js", "mediabunny.min.mjs", "mediabunny.LICENSE")

Write-Host "== TPT Video Compressor build ==" -ForegroundColor Cyan

if ($Pro) {
    Write-Host " edition: pro (saved presets + batch folders + no length limits)"
    Write-Host "-- trunk build --release --features pro (standalone bundle for the desktop shell)" -ForegroundColor Cyan
    trunk build --release --features pro
    if ($LASTEXITCODE -ne 0) { throw "trunk build failed" }

    Write-Host "-- cargo build --release -p tpt-video-desktop (webview exe)" -ForegroundColor Cyan
    cargo build --release -p tpt-video-desktop
    if ($LASTEXITCODE -ne 0) { throw "desktop build failed" }
    Write-Host "   exe: target\release\tpt-video-compressor-pro.exe (embeds dist\)" -ForegroundColor Cyan
    Write-Host "-- done" -ForegroundColor Green
    Write-Host "   (public/apps/video-compressor/ was not touched - run ./build.ps1 with no flags to refresh the free demo)"
    exit 0
}

# Where the browser-loadable free bundle goes.
$WebRepo = if ($env:TPT_WEB_REPO) { $env:TPT_WEB_REPO } elseif (Test-Path $DefaultWebRepo) { $DefaultWebRepo } else { $null }
$Dest = if ($WebRepo) {
    Join-Path $WebRepo "public\apps\$Slug"
} else {
    Join-Path $PSScriptRoot "dist\web"
}

Write-Host " edition: free (one clip, no saved presets)"
Write-Host " dest:    $Dest"

if (-not $SkipBuild) {
    Write-Host "-- cargo build --release --target $Target" -ForegroundColor Cyan
    # --features hub: asset URLs become site-absolute /apps/video-compressor/...
    cargo build --release --target $Target --features hub -p $Crate
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}

$Wasm = Join-Path $PSScriptRoot "target\$Target\release\tpt_app_video_preset_compressor.wasm"
if (-not (Test-Path $Wasm)) { throw "missing $Wasm - run the cargo build step first" }

Write-Host "-- wasm-bindgen --target web --out-name $OutName" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $Dest | Out-Null
wasm-bindgen --target web --out-name $OutName --out-dir $Dest $Wasm
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen failed" }

$Glue = Join-Path $Dest "$OutName.js"
$BgWasm = Join-Path $Dest "$($OutName)_bg.wasm"
foreach ($artifact in @($Glue, $BgWasm)) {
    if (-not (Test-Path $artifact)) { throw "wasm-bindgen did not produce $artifact" }
}

Write-Host "-- copy engine runtime (flat, next to the glue)" -ForegroundColor Cyan
foreach ($asset in $EngineAssets) {
    $source = Join-Path $PSScriptRoot "assets\$asset"
    if (-not (Test-Path $source)) { throw "missing vendored asset $source" }
    Copy-Item -Force $source (Join-Path $Dest $asset)
}

# Size report: the app's own wasm payload (tuned with opt-level = z, lto,
# strip). mediabunny is a small JS module and is reported by its file size.
Add-Type -AssemblyName System.IO.Compression
function Get-Gzip-Bytes([string]$Path) {
    $rawBytes = [System.IO.File]::ReadAllBytes($Path)
    $memoryStream = [System.IO.MemoryStream]::new()
    $gzipStream = [System.IO.Compression.GZipStream]::new($memoryStream, [System.IO.Compression.CompressionLevel]::Optimal)
    $gzipStream.Write($rawBytes, 0, $rawBytes.Length)
    $gzipStream.Dispose()
    $gzipped = $memoryStream.ToArray().Length
    $memoryStream.Dispose()
    return $gzipped
}

$appRaw = (Get-Item $BgWasm).Length
$appGz = Get-Gzip-Bytes $BgWasm
Write-Host "-- app payload $($OutName)_bg.wasm: $appRaw bytes raw, $appGz bytes gzipped ($([math]::Round($appGz / 1KB, 1)) KB gz)" -ForegroundColor Cyan
if ($appGz -gt 2MB) {
    Write-Warning "app payload exceeds the ~2 MB gzip budget"
}

$mbRaw = (Get-Item (Join-Path $Dest "mediabunny.min.mjs")).Length
$mbGz = Get-Gzip-Bytes (Join-Path $Dest "mediabunny.min.mjs")
Write-Host "-- mediabunny.min.mjs (vendored): $mbRaw bytes raw, $mbGz bytes gzipped ($([math]::Round($mbGz / 1KB, 1)) KB gz)" -ForegroundColor Cyan

Write-Host "-- done" -ForegroundColor Green
if (-not $WebRepo) {
    Write-Host "   (set TPT_WEB_REPO to copy the bundle into the hub web repo)"
} else {
    Write-Host ""
    Write-Host "Paste into src/lib/tpt-apps-registry.ts (TPT_APPS_REGISTRY), then flip status to 'live':" -ForegroundColor Cyan
    Write-Host @"
  {
    slug: '$Slug',
    name: 'TPT Video Compressor',
    description: 'Compress and convert video with saved per-client presets - batch folders in the Pro desktop edition.',
    category: 'utility',
    status: 'coming-soon',
    kind: 'demo',
    wasm: { entry: '/apps/$Slug/$OutName.js' },
    // gumroadUrl: 'https://tpt12.gumroad.com/l/...',  // add once the `$19 Pro exe is listed
    price: '`$19',
  },
"@
}
