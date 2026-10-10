//! TPT Video Compressor engine model.
//!
//! Pure, target-independent Rust: the export-preset model, the
//! [`encode::EncodeSpec`] handed to the browser-side WebCodecs engine
//! (`assets/engine.js`), and the [`media::MediaInfo`] it reports back. No
//! wasm or DOM code lives here, so the whole crate runs as ordinary host
//! tests (`cargo test -p tpt-video-engine`).
//!
//! Royalty-free policy: the only output codecs that exist in this model are
//! AV1 / VP9 / VP8 video and Opus audio, in WebM.

pub mod encode;
pub mod media;
pub mod preset;

pub use preset::ExportPreset;
