#!/usr/bin/env sh
# TPT Video Compressor - build pipeline (macOS/Linux).
#
# Free edition (default):  ./build.sh
# Pro desktop edition:     ./build.sh --pro
#   (adds trunk build --release --features pro + the native webview exe)
#
# Env:
#   TPT_WEB_REPO  path to the web repo; the bundle is copied to
#                 $TPT_WEB_REPO/public/apps/video-compressor/ (glue named
#                 video-compressor.js, wasm keeps its _bg.wasm name, and
#                 the vendored engine (engine.js + mediabunny) is copied
#                 flat next to them), otherwise dist/web in this repo.

set -eu
cd "$(dirname "$0")"

CRATE=tpt-app-video-preset-compressor
TARGET=wasm32-unknown-unknown
OUT_NAME=video-compressor
ENGINE_ASSETS="engine.js mediabunny.min.mjs mediabunny.LICENSE"

PRO=0
for arg in "$@"; do
  case "$arg" in
    --pro) PRO=1 ;;
    *) echo "usage: build.sh [--pro]" >&2; exit 2 ;;
  esac
done

if [ -n "${TPT_WEB_REPO:-}" ]; then
  DEST="$TPT_WEB_REPO/public/apps/video-compressor"
else
  DEST="dist/web"
fi

echo "== TPT Video Compressor build =="
echo " edition: $([ "$PRO" = 1 ] && echo 'pro (saved presets + batch folders + no length limits)' || echo 'free (one clip, no saved presets)')"
echo " dest:    $DEST"

if [ "$PRO" = 1 ]; then
  trunk build --release --features pro
  cargo build --release -p tpt-video-desktop
  echo "   exe: target/release/tpt-video-compressor-pro (embeds dist/)"
  echo "-- done"
  echo "   (public/apps/video-compressor/ was not touched - run ./build.sh with no flags to refresh the free demo)"
  exit 0
fi

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  # --features hub: asset URLs become site-absolute /apps/video-compressor/...
  cargo build --release --target "$TARGET" --features hub -p "$CRATE"
fi

WASM="target/$TARGET/release/tpt_app_video_preset_compressor.wasm"
[ -f "$WASM" ] || { echo "missing $WASM" >&2; exit 1; }

mkdir -p "$DEST"
wasm-bindgen --target web --out-name "$OUT_NAME" --out-dir "$DEST" "$WASM"

GLUE="$DEST/$OUT_NAME.js"
BG_WASM="$DEST/${OUT_NAME}_bg.wasm"
[ -f "$GLUE" ] || { echo "wasm-bindgen did not produce $GLUE" >&2; exit 1; }
[ -f "$BG_WASM" ] || { echo "wasm-bindgen did not produce $BG_WASM" >&2; exit 1; }

for asset in $ENGINE_ASSETS; do
  [ -f "assets/$asset" ] || { echo "missing vendored asset assets/$asset" >&2; exit 1; }
  cp -f "assets/$asset" "$DEST/$asset"
done

# Size report - the app's own payload (see the Windows script).
RAW=$(wc -c < "$BG_WASM" | tr -d ' ')
if command -v gzip >/dev/null 2>&1; then
  GZ=$(gzip -9 -c "$BG_WASM" | wc -c | tr -d ' ')
  echo "-- app payload ${OUT_NAME}_bg.wasm: $RAW bytes raw, $GZ bytes gzipped"
  if [ "$GZ" -gt 2097152 ]; then
    echo "warning: app payload exceeds the ~2 MB gzip budget" >&2
  fi
fi

echo "-- done"
[ -n "${TPT_WEB_REPO:-}" ] || echo "   (set TPT_WEB_REPO to copy the bundle into the hub web repo)"
