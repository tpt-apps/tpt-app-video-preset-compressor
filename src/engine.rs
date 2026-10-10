//! JS interop with the browser-side transcoding engine (`assets/engine.js`).
//!
//! The engine runs on the browser's own WebCodecs (decode H.264/HEVC/AAC via
//! the browser/OS licensed decoders; encode AV1/VP9/VP8 + Opus into WebM),
//! through the vendored mediabunny library. No codec code ships with this
//! app — see `assets/README.md` for the royalty-free policy.
//!
//! `engine.js` is an ES module loaded with a dynamic `import()`; it resolves
//! `mediabunny.min.mjs` relative to its own URL, so both files must ship
//! flat in the same directory in every hosting context.
//!
//! Asset URLs are [`build_asset_url`]'s job, and the two build flavors differ
//! on purpose:
//! - `--features hub` (the free bundle `build.ps1` ships to the hub) uses
//!   the site-absolute `/apps/video-compressor/` prefix, because the hub
//!   page lives at `/tools/video-compressor` and page-relative URLs would
//!   miss the app's asset directory;
//! - plain builds (trunk dev serve, and the Pro desktop bundle that embeds
//!   `dist/`) use page-relative names, because in both of those hosts the
//!   page and all assets sit in the same served root.

use std::rc::Rc;

use js_sys::{Function, Promise, Reflect, Uint8Array};
use tpt_video_engine::encode::EncodeSpec;
use tpt_video_engine::media::MediaInfo;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// URL of one of the vendored runtime files in the currently-built flavor.
pub fn build_asset_url(filename: &str) -> String {
    #[cfg(feature = "hub")]
    {
        format!("/apps/video-compressor/{filename}")
    }
    #[cfg(not(feature = "hub"))]
    {
        filename.to_string()
    }
}

/// A finished compression.
pub struct Compressed {
    pub blob: web_sys::Blob,
    /// The video codec actually used (`"av1"`, `"vp9"` or `"vp8"`) — may
    /// differ from the request when the browser can't encode the preferred
    /// one.
    pub codec: String,
}

/// A loaded engine module (its namespace object).
pub struct Engine {
    /// A plain `JsValue`, not `Object`: a module namespace has a null
    /// prototype, so an `instanceof Object` check (`dyn_into`) rejects it.
    module: JsValue,
}

impl Engine {
    /// Imports `engine.js` and checks that this browser can run it.
    pub async fn load() -> Result<Self, String> {
        let importer = Function::new_with_args(
            "url",
            "return import(new URL(url, document.baseURI).href)",
        );
        let promise = importer
            .call1(&JsValue::NULL, &JsValue::from_str(&build_asset_url("engine.js")))
            .map_err(|e| js_message(&e))?;
        let module = JsFuture::from(promise.dyn_into::<Promise>().map_err(|_| "import did not return a promise")?)
            .await
            .map_err(|e| format!("could not load the engine: {}", js_message(&e)))?;
        if !module.is_object() {
            return Err("engine module is not an object".to_string());
        }
        let engine = Self { module };

        let caps = engine.call("capabilities", &[]).await?;
        let supported = Reflect::get(&caps, &JsValue::from_str("supported"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !supported {
            let reason = Reflect::get(&caps, &JsValue::from_str("reason"))
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_else(|| "this browser is not supported".to_string());
            return Err(reason);
        }
        Ok(engine)
    }

    /// Reads the clip's metadata. Errors when the file isn't readable video.
    pub async fn probe(&self, file: &web_sys::File) -> Result<MediaInfo, String> {
        let value = self.call("probe", &[file.clone().into()]).await?;
        let json = js_sys::JSON::stringify(&value)
            .map_err(|e| js_message(&e))?
            .as_string()
            .unwrap_or_default();
        MediaInfo::from_json(&json).map_err(|e| format!("unexpected probe result: {e}"))
    }

    /// Compresses `file` per `spec`. `watermark_png` must be `Some` when the
    /// spec carries a watermark. `on_progress` receives 0..=1. When `output`
    /// is a `FileSystemFileHandle` the WebM streams straight into that file
    /// (see `compress()` in engine.js); `None` buffers it in memory.
    pub async fn compress(
        &self,
        file: &web_sys::File,
        spec: &EncodeSpec,
        watermark_png: Option<&[u8]>,
        on_progress: Rc<dyn Fn(f64)>,
        output: Option<JsValue>,
    ) -> Result<Compressed, String> {
        let spec_json = serde_json::to_string(spec).map_err(|e| e.to_string())?;
        let spec_js = js_sys::JSON::parse(&spec_json).map_err(|e| js_message(&e))?;
        let wm_js = match watermark_png {
            Some(bytes) => JsValue::from(Uint8Array::from(bytes)),
            None => JsValue::NULL,
        };
        let progress = Closure::wrap(Box::new(move |ratio: f64| on_progress(ratio)) as Box<dyn FnMut(f64)>);
        let result = self
            .call(
                "compress",
                &[
                    file.clone().into(),
                    spec_js,
                    wm_js,
                    progress.as_ref().clone(),
                    output.unwrap_or(JsValue::UNDEFINED),
                ],
            )
            .await;
        drop(progress);
        let result = result?;

        let blob = Reflect::get(&result, &JsValue::from_str("blob"))
            .map_err(|e| js_message(&e))?
            .dyn_into::<web_sys::Blob>()
            .map_err(|_| "engine returned no output".to_string())?;
        let codec = Reflect::get(&result, &JsValue::from_str("codec"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        Ok(Compressed { blob, codec })
    }

    /// Cancels the running compression, if any (best effort).
    #[allow(dead_code)]
    pub async fn cancel(&self) {
        let _ = self.call("cancel", &[]).await;
    }

    async fn call(&self, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
        let function = Reflect::get(&self.module, &JsValue::from_str(name))
            .map_err(|e| js_message(&e))?
            .dyn_into::<Function>()
            .map_err(|_| format!("engine.{name} is missing"))?;
        let argv = js_sys::Array::new();
        for arg in args {
            argv.push(arg);
        }
        let ret = Reflect::apply(&function, &JsValue::UNDEFINED, &argv).map_err(|e| js_message(&e))?;
        match ret.dyn_into::<Promise>() {
            Ok(promise) => JsFuture::from(promise).await.map_err(|e| js_message(&e)),
            Err(plain) => Ok(plain),
        }
    }
}

/// Best-effort text of a thrown JS value (an `Error`, a string, …).
fn js_message(value: &JsValue) -> String {
    if let Some(error) = value.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    if let Some(text) = value.as_string() {
        return text;
    }
    if let Ok(message) = Reflect::get(value, &JsValue::from_str("message")) {
        if let Some(text) = message.as_string() {
            return text;
        }
    }
    format!("{value:?}")
}
