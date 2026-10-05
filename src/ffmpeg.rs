//! JS interop with the vendored ffmpeg.wasm runtime (`assets/`).
//!
//! Architecture: the UMD wrapper (`ffmpeg.js`, global `FFmpegWASM`) is
//! injected with a plain `<script>` tag. Do NOT pass `classWorkerURL` to
//! `load()`: in the 0.12.15 UMD build that combination spawns a *module*
//! worker whose dynamic-import fallback is a webpack stub that always
//! throws ("Cannot find module"). Without it the wrapper spawns its own
//! *classic* worker chunk (`814.ffmpeg.js`), resolved from the injected
//! script's own directory, and a classic worker loads the core via
//! `importScripts` — which only works with the UMD core build (plain
//! script, no `export` statements). Hence `assets/` ships the UMD core.
//! All video work happens in that worker, so long encodes never block
//! the UI thread.
//!
//! Asset URLs are `build_asset_url`'s job, and the two build flavors differ
//! on purpose:
//! - `--features hub` (the free bundle `build.ps1` ships to the hub) uses
//!   the site-absolute `/apps/video-compressor/` prefix, because the hub
//!   page lives at `/tools/video-compressor` and page-relative URLs would
//!   miss the app's asset directory;
//! - plain builds (trunk dev serve, and the Pro desktop bundle that embeds
//!   `dist/`) use page-relative names, because in both of those hosts the
//!   page and all assets sit in the same served root.
//! In every flavor the four ffmpeg files must ship flat, next to the glue,
//! with the wrapper injected from that same directory (the wrapper derives
//! its worker URL from the script tag's src).

use std::rc::Rc;

use js_sys::{Array, Function, Object, Promise, Reflect, Uint8Array};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Receives `(type, message)` log lines from the ffmpeg core — `type` is
/// ffmpeg's stream tag ("stderr", "stdout", "info", …). The probe parser
/// consumes the stderr text.
pub type LogCallback = Rc<dyn Fn(&str, &str)>;

/// Receives ffmpeg's progress events: `(ratio, time_micros)`. `ratio` is
/// the core's own estimate (relative to the input duration it detected;
/// can exceed 1.0 or be 0 when unknown) — the app clamps/recomputes it.
pub type ProgressCallback = Rc<dyn Fn(f64, f64)>;

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

/// A loaded, ready-to-use ffmpeg worker connection.
pub struct FFmpeg {
    instance: Object,
    // The event closures are registered once at load and must outlive
    // every call — held here so dropping the connection (process lifetime
    // in practice) releases them.
    _log_closure: Closure<dyn FnMut(JsValue)>,
    _progress_closure: Closure<dyn FnMut(JsValue)>,
}

impl FFmpeg {
    /// Loads the runtime (idempotent) and boots one worker with the core
    /// inside. Later calls reuse the same script tag but spawn a fresh
    /// worker per `FFmpeg` value — the app keeps exactly one.
    pub async fn load(log: LogCallback, progress: ProgressCallback) -> Result<Self, JsValue> {
        ensure_runtime().await?;

        let ctor = {
            let global = js_sys::global();
            let wrapper = Reflect::get(&global, &JsValue::from_str("FFmpegWASM"))?;
            if wrapper.is_undefined() {
                return Err(JsValue::from_str(
                    "ffmpeg.wasm wrapper did not load (FFmpegWASM global missing)",
                ));
            }
            Reflect::get(&wrapper, &JsValue::from_str("FFmpeg"))?.dyn_into::<Function>()?
        };
        let instance: Object = Reflect::construct(&ctor, &Array::new())
            .map_err(|_| JsValue::from_str("FFmpeg constructor threw"))?
            .dyn_into()
            .map_err(|_| JsValue::from_str("FFmpeg instance is not an object"))?;

        let log_closure = register_event(&instance, "log", move |data| {
            if let (Some(kind), Some(message)) = (
                Reflect::get(&data, &JsValue::from_str("type"))
                    .ok()
                    .and_then(|v| v.as_string()),
                Reflect::get(&data, &JsValue::from_str("message"))
                    .ok()
                    .and_then(|v| v.as_string()),
            ) {
                log(&kind, &message);
            }
        });
        let progress_closure = register_event(&instance, "progress", move |data| {
            let ratio = Reflect::get(&data, &JsValue::from_str("progress"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let time = Reflect::get(&data, &JsValue::from_str("time"))
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            progress(ratio, time);
        });

        // Unambiguous URLs for the UMD core pair. Deliberately no
        // `classWorkerURL` — see the module docs on why the wrapper's own
        // classic-worker path is the only working combination here. The
        // worker chunk is located from the injected script tag's src, so
        // the wrapper must be injected from the same directory as these.
        let config = Object::new();
        for (key, file) in [
            ("coreURL", "ffmpeg-core.js"),
            ("wasmURL", "ffmpeg-core.wasm"),
        ] {
            Reflect::set(
                &config,
                &JsValue::from_str(key),
                &JsValue::from_str(&build_asset_url(file)),
            )?;
        }

        let load_fn = Reflect::get(&instance, &JsValue::from_str("load"))?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str("FFmpeg.load missing"))?;
        let promise = load_fn.call1(&instance, &config)?;
        JsFuture::from(promise.dyn_into::<Promise>()?).await?;

        Ok(Self {
            instance,
            _log_closure: log_closure,
            _progress_closure: progress_closure,
        })
    }

    /// Writes a virtual in-memory file into the worker's FS (the input
    /// clip, the watermark PNG).
    pub async fn write_file(&self, name: &str, data: &[u8]) -> Result<(), JsValue> {
        let method = self.method("writeFile")?;
        let promise = method.call2(
            &self.instance,
            &JsValue::from_str(name),
            &Uint8Array::from(data),
        )?;
        JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        Ok(())
    }

    /// Runs ffmpeg with the given arguments in the worker. Returns the
    /// process exit code (0 = success; e.g. a bare `-i` probe exits 1 by
    /// design because no output file was requested).
    pub async fn exec(&self, args: &[&str]) -> Result<i32, JsValue> {
        let method = self.method("exec")?;
        let argv = Array::new();
        for arg in args {
            argv.push(&JsValue::from_str(arg));
        }
        let promise = method.call1(&self.instance, &argv)?;
        let ret = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        Ok(ret.as_f64().unwrap_or(-1.0) as i32)
    }

    /// Reads a virtual file back out (the compressed output).
    pub async fn read_file(&self, name: &str) -> Result<Vec<u8>, JsValue> {
        let method = self.method("readFile")?;
        let promise = method.call1(&self.instance, &JsValue::from_str(name))?;
        let result = JsFuture::from(promise.dyn_into::<Promise>()?).await?;
        Ok(Uint8Array::from(result).to_vec())
    }

    /// Best-effort delete of a virtual file (keeps worker memory flat
    /// across batch runs); failures are ignored on purpose.
    pub fn delete_file(&self, name: &str) {
        if let Ok(method) = self.method("deleteFile") {
            let _ = method.call1(&self.instance, &JsValue::from_str(name));
        }
    }

    fn method(&self, name: &str) -> Result<Function, JsValue> {
        Reflect::get(&self.instance, &JsValue::from_str(name))?
            .dyn_into::<Function>()
            .map_err(|_| JsValue::from_str(&format!("FFmpeg.{name} missing")))
    }
}

fn register_event(
    instance: &Object,
    event: &'static str,
    handler: impl Fn(JsValue) + 'static,
) -> Closure<dyn FnMut(JsValue)> {
    let closure =
        Closure::wrap(Box::new(move |data: JsValue| handler(data)) as Box<dyn FnMut(JsValue)>);
    if let Ok(on) = Reflect::get(instance, &JsValue::from_str("on"))
        .and_then(|f| f.dyn_into::<Function>())
    {
        let _ =
            on.call2(instance, &JsValue::from_str(event), closure.as_ref().unchecked_ref());
    }
    closure
}

/// True once the UMD wrapper global exists.
pub fn runtime_ready() -> bool {
    let global = js_sys::global();
    Reflect::get(&global, &JsValue::from_str("FFmpegWASM"))
        .map(|v| !v.is_undefined() && !v.is_null())
        .unwrap_or(false)
}

/// Injects the UMD wrapper `<script>` once and resolves when the
/// `FFmpegWASM` global is available.
async fn ensure_runtime() -> Result<(), JsValue> {
    if runtime_ready() {
        return Ok(());
    }
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;

    let existing = document.query_selector("script[data-tpt-vc-ffmpeg]")?;
    if existing.is_none() {
        let script: web_sys::HtmlScriptElement = document
            .create_element("script")?
            .dyn_into()
            .map_err(|_| JsValue::from_str("script element cast failed"))?;

        let promise = Promise::new(&mut |resolve, reject| {
            let on_ready = Closure::once_into_js(move || {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            });
            let on_fail = Closure::once_into_js(move || {
                let _ = reject.call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str("ffmpeg.js failed to load"),
                );
            });
            script.set_onload(Some(on_ready.unchecked_ref()));
            script.set_onerror(Some(on_fail.unchecked_ref()));
            script.set_src(&build_asset_url("ffmpeg.js"));
            script.set_async(true);
            let _ = script.set_attribute("data-tpt-vc-ffmpeg", "1");
            if let Some(head) = document.query_selector("head").ok().flatten() {
                let _ = head.append_child(&script);
            }
        });
        JsFuture::from(promise).await?;
    } else {
        // Another mount injected the tag already — poll until its load
        // lands (or time out).
        for _ in 0..200 {
            if runtime_ready() {
                break;
            }
            sleep_ms(50).await;
        }
    }

    if !runtime_ready() {
        return Err(JsValue::from_str(
            "ffmpeg.js loaded but FFmpegWASM global is missing",
        ));
    }
    Ok(())
}

async fn sleep_ms(ms: i32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let promise = Promise::new(&mut |resolve: Function, _| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            &resolve,
            ms,
        );
    });
    let _ = JsFuture::from(promise).await;
}
