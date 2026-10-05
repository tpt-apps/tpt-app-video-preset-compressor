//! TPT Video Compressor engine.
//!
//! Pure, target-independent Rust: the export-preset model, the ffmpeg
//! argument builder that turns a preset into a command line for the
//! ffmpeg.wasm core, and the parser that reads media metadata back out of
//! `ffmpeg -i` log output. No wasm or DOM code lives here, so the whole
//! crate runs as ordinary host tests (`cargo test -p tpt-video-engine`).

pub mod cmd;
pub mod preset;
pub mod probe;

pub use preset::ExportPreset;
