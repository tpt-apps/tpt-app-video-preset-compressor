# TPT Video Compressor - task runner.
# The pipeline logic lives in build.ps1 (Windows) / build.sh (unix); these
# recipes are thin wrappers so `just` works everywhere.

set windows-shell := ["pwsh.exe", "-NoLogo", "-NoProfile", "-Command"]

default:
    @just --list

# Build the free WASM edition and report the payload sizes.
build:
    pwsh -NoLogo -NoProfile -File build.ps1

# Build the Pro edition: saved presets/batch/no-limits + trunk bundle + desktop exe.
pro:
    pwsh -NoLogo -NoProfile -File build.ps1 -Pro

# Re-run only wasm-bindgen + asset copy (after an existing release build).
bindgen:
    pwsh -NoLogo -NoProfile -File build.ps1 -SkipBuild

# Engine units (preset model / encode spec / media info), then type-check
# the wasm lib (free) and the desktop shell.
test:
    cargo test -p tpt-video-engine
    cargo check --target wasm32-unknown-unknown -p tpt-app-video-preset-compressor

# Type-check everything: wasm lib (free + pro) and the desktop shell.
check:
    cargo check --target wasm32-unknown-unknown -p tpt-app-video-preset-compressor
    cargo check --target wasm32-unknown-unknown --features pro -p tpt-app-video-preset-compressor
    cargo check -p tpt-video-desktop

# Live-reload standalone dev server (index.html mounts via #tpt-appfront-root).
dev:
    trunk serve

# Remove build outputs.
clean:
    cargo clean
    -rd /s /q dist 2>nul || rm -rf dist
