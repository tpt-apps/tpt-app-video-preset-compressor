//! Free/Pro DOM UI (tpt-appfront-dom).
//!
//! Architecture (mirrors tpt-app-fea-lite's proven split):
//! - the **shell is mounted once** (`tpt_appfront_dom::mount`) — settings
//!   keep focus while typing; their values are read straight from the DOM
//!   when Compress is pressed;
//! - the **source/result sections re-render fine-grained**
//!   (`tpt_appfront_dom::render`) driven by the `source`/`job` Signals;
//! - everything the DOM backend has no node kind for (file inputs, the
//!   drop zone, the progress bar, the video previews, the watermark picker)
//!   is raw web-sys appended into placeholder containers, wired with
//!   `Closure`s, exactly like fea-lite's canvas viewer.
//!
//! The engine (preset model, ffmpeg argument building, probe-log parsing)
//! lives in `tpt-video-engine`; ffmpeg itself runs in the vendored
//! ffmpeg.wasm worker behind `crate::ffmpeg`.

use std::cell::RefCell;
use std::rc::Rc;

use tpt_appfront_core::{create_effect, Signal, UITree};
use tpt_video_engine::cmd;
use tpt_video_engine::preset::{Audio, Container, ExportPreset, Quality};
#[cfg(feature = "pro")]
use tpt_video_engine::preset::Corner;
use tpt_video_engine::probe::{parse_probe_log, MediaInfo};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::ffmpeg::FFmpeg;

#[cfg(feature = "pro")]
use tpt_video_engine::preset::Watermark;

/// Free edition clip limits (Pro removes both — the difference the $19
/// listing's "no file-size/length limits" bullet points at).
#[cfg(not(feature = "pro"))]
const FREE_MAX_INPUT_BYTES: u64 = 200 * 1024 * 1024; // 200 MiB
#[cfg(not(feature = "pro"))]
const FREE_MAX_DURATION_SECS: f64 = 5.0 * 60.0;

/// Virtual filenames inside the ffmpeg worker's in-memory FS.
const INPUT_NAME: &str = "input.clip";
#[cfg(feature = "pro")]
const WATERMARK_FS_NAME: &str = cmd::WATERMARK_FILENAME;

/// The currently selected source clip (free edition: the one clip).
#[derive(Clone)]
struct SourceClip {
    file_name: String,
    size_bytes: u64,
    /// Blob URL of the original file for the <video> preview.
    preview_url: String,
    /// Filled once the engine has probed the file; `None` until then.
    info: Option<Rc<MediaInfo>>,
}

/// The result section's current content.
#[derive(Clone)]
enum Job {
    Idle,
    /// Engine loading / probing / writing — indeterminate.
    Working(String),
    /// Encoding with a 0..=1 progress ratio.
    Encoding(f64),
    Done(Rc<JobOutcome>),
    Failed(String),
    /// Free-edition limit hit; the message doubles as the upsell hook.
    #[cfg(not(feature = "pro"))]
    LimitHit(String),
}

struct JobOutcome {
    /// Blob URL of the compressed file (drives preview + download).
    output_url: String,
    output_name: String,
    output_size: u64,
    input_size: u64,
    container_label: &'static str,
}

#[derive(Debug, Clone)]
enum Msg {
    /// Open the OS file picker for the watermark PNG. (pro)
    #[cfg(feature = "pro")]
    PickWatermark,
    /// (pro) Queue more files via the multi-file picker.
    #[cfg(feature = "pro")]
    BatchPickFiles,
    /// (pro) Queue a whole folder via the directory picker.
    #[cfg(feature = "pro")]
    BatchPickFolder,
    /// (pro) Save the current settings under the name in the name field.
    #[cfg(feature = "pro")]
    SavePreset,
    /// (pro) Apply the saved preset at `index`.
    #[cfg(feature = "pro")]
    ApplyPreset(usize),
    /// (pro) Delete the saved preset at `index`.
    #[cfg(feature = "pro")]
    DeletePreset(usize),
    /// A platform quick-fill was selected; fill the fields from it.
    PlatformChosen(String),
    /// Compress the source clip with the current settings.
    Compress,
    /// (pro) Compress every pending clip in the batch queue.
    #[cfg(feature = "pro")]
    BatchStart,
    /// Download the most recent single-clip result.
    DownloadResult,
    /// (pro) Download the batch output stored at `slot`.
    #[cfg(feature = "pro")]
    DownloadBatch(usize),
    /// Open a TPT Solutions URL — new browser tab in the free edition, or
    /// via the desktop host's system-browser IPC action in the Pro shell.
    /// See `visit_site`.
    VisitSite(String),
}

// The app root element, stashed so message handlers (which carry no
// element references) can reach the form.
thread_local! {
    static CONTAINER: RefCell<Option<web_sys::Element>> = const { RefCell::new(None) };
}

fn current_container() -> Option<web_sys::Element> {
    CONTAINER.with(|slot| slot.borrow().clone())
}

/// Mounts the whole app into `container` (the hub's container div, or the
/// standalone `#tpt-appfront-root` marker).
pub fn mount_app(container: &web_sys::Element) -> Result<(), JsValue> {
    let document = web_sys::window()
        .expect("no window")
        .document()
        .expect("no document");

    upsert_style(&document, "tpt-vc-styles", BASE_CSS);
    // `.vc-app` on the mount root itself: every color/font custom property
    // lives here, so the theme depends on this class being present on an
    // ancestor of the tree the CSS above targets.
    let existing_class = container.get_attribute("class").unwrap_or_default();
    if !existing_class.split_whitespace().any(|c| c == "vc-app") {
        let combined = if existing_class.is_empty() {
            "vc-app".to_string()
        } else {
            format!("{existing_class} vc-app")
        };
        let _ = container.set_attribute("class", &combined);
    }
    CONTAINER.with(|slot| *slot.borrow_mut() = Some(container.clone()));

    let source: Signal<Option<Rc<SourceClip>>> = Signal::new(None);
    let job: Signal<Job> = Signal::new(Job::Idle);
    #[cfg(feature = "pro")]
    let presets: Signal<Vec<ExportPreset>> = Signal::new(load_saved_presets());
    #[cfg(feature = "pro")]
    let queue: Signal<Vec<QueueItem>> = Signal::new(Vec::new());
    #[cfg(feature = "pro")]
    let batch: Signal<BatchState> = Signal::new(BatchState::Idle);

    // Hidden inputs backing the pick buttons, created up front so the
    // dispatch closure can `.click()` them. Their change listeners are
    // wired after the shell mount, below.
    let file_input = hidden_input(&document, container, "video/*,.mp4,.mov,.mkv,.webm,.avi,.m4v", false)?;
    #[cfg(feature = "pro")]
    let watermark_input = hidden_input(&document, container, "image/png", false)?;
    #[cfg(feature = "pro")]
    let batch_input = hidden_input(&document, container, "video/*,.mp4,.mov,.mkv,.webm,.avi,.m4v", true)?;
    #[cfg(feature = "pro")]
    let folder_input = hidden_input(&document, container, "", true)?;
    #[cfg(feature = "pro")]
    folder_input.set_attribute("webkitdirectory", "")?;

    let dispatch: Rc<dyn Fn(Msg)> = {
        let source = source.clone();
        let job = job.clone();
        #[cfg(feature = "pro")]
        let watermark_input = watermark_input.clone();
        #[cfg(feature = "pro")]
        let batch_input = batch_input.clone();
        #[cfg(feature = "pro")]
        let folder_input = folder_input.clone();
        #[cfg(feature = "pro")]
        let presets = presets.clone();
        #[cfg(feature = "pro")]
        let queue = queue.clone();
        #[cfg(feature = "pro")]
        let batch = batch.clone();
        Rc::new(move |msg| match msg {
            #[cfg(feature = "pro")]
            Msg::PickWatermark => {
                let _ = watermark_input.click();
            }
            #[cfg(feature = "pro")]
            Msg::BatchPickFiles => {
                let _ = batch_input.click();
            }
            #[cfg(feature = "pro")]
            Msg::BatchPickFolder => {
                let _ = folder_input.click();
            }
            #[cfg(feature = "pro")]
            Msg::SavePreset => {
                if let Some(container) = current_container() {
                    save_preset(&container, &presets);
                }
            }
            #[cfg(feature = "pro")]
            Msg::ApplyPreset(index) => {
                if let Some(container) = current_container() {
                    apply_saved_preset(&container, &presets, index);
                }
            }
            #[cfg(feature = "pro")]
            Msg::DeletePreset(index) => delete_saved_preset(&presets, index),
            Msg::PlatformChosen(label) => {
                if let Some(container) = current_container() {
                    apply_platform_preset(&container, &label);
                }
            }
            Msg::Compress => run_compress(&source, &job),
            #[cfg(feature = "pro")]
            Msg::BatchStart => start_batch(&queue, &batch, &job),
            Msg::DownloadResult => {
                if let Some(outcome) = LAST_OUTPUT.with(|slot| slot.borrow().clone()) {
                    trigger_download(&outcome);
                }
            }
            #[cfg(feature = "pro")]
            Msg::DownloadBatch(slot_index) => {
                let target = BATCH_OUTPUTS.with(|slot| slot.borrow().get(slot_index).cloned());
                if let Some(outcome) = target {
                    trigger_download(&outcome);
                }
            }
            Msg::VisitSite(url) => visit_site(&url),
        })
    };

    // 1. Static shell — mounted exactly once.
    let shell = shell_tree();
    let root = tpt_appfront_dom::mount(container, &shell, dispatch.clone())?;
    std::mem::forget(root);

    // 2. Reactive source meta (file info / limits) into its placeholder.
    let source_el = container
        .query_selector(".vc-source-meta")?
        .ok_or_else(|| JsValue::from_str("source meta placeholder missing"))?;
    {
        let source = source.clone();
        let job = job.clone();
        let handle = tpt_appfront_dom::render(
            &source_el,
            Rc::new(move || source_meta_view(source.get(), job.get())),
            dispatch.clone(),
        )?;
        std::mem::forget(handle);
    }

    // 3. Reactive result section into its placeholder.
    let results_el = container
        .query_selector(".vc-results")?
        .ok_or_else(|| JsValue::from_str("results placeholder missing"))?;
    {
        let job = job.clone();
        let handle = tpt_appfront_dom::render(
            &results_el,
            Rc::new(move || results_view(job.get())),
            dispatch.clone(),
        )?;
        std::mem::forget(handle);
    }

    // 4. Raw-DOM extras: drop zone + pickers.
    wire_drop_zone(container, &file_input)?;
    wire_source_input(&file_input, &source, &job);
    #[cfg(feature = "pro")]
    {
        wire_watermark_input(&watermark_input);
        wire_batch_inputs(&batch_input, &queue);
        wire_batch_inputs(&folder_input, &queue);
    }

    // 5. Source <video> preview + progress bar: raw nodes kept in sync by
    // effects over the signals.
    {
        let source = source.clone();
        let container = container.clone();
        let handle = create_effect(move || {
            let clip = source.get();
            if let Some(holder) = container
                .query_selector(".vc-source-preview")
                .ok()
                .flatten()
            {
                let media = clip.as_deref().map(|c| (c.preview_url.clone(), c.file_name.clone()));
                let _ = update_video_preview(&holder, media);
            }
        });
        std::mem::forget(handle);
    }
    {
        let job = job.clone();
        let container = container.clone();
        let handle = create_effect(move || {
            let ratio = match job.get() {
                Job::Encoding(p) => Some(p),
                Job::Working(_) => Some(0.0),
                _ => None,
            };
            let _ = update_progress_bar(&container, ratio);
            let _ = set_compress_busy(&container, matches!(job.get(), Job::Working(_) | Job::Encoding(_)));
        });
        std::mem::forget(handle);
    }

    // 6. (pro) Reactive preset manager + batch queue sections.
    #[cfg(feature = "pro")]
    {
        let presets_el = container
            .query_selector(".vc-presets")?
            .ok_or_else(|| JsValue::from_str("presets placeholder missing"))?;
        let presets_for_render = presets.clone();
        let handle = tpt_appfront_dom::render(
            &presets_el,
            Rc::new(move || presets_view(presets_for_render.get())),
            dispatch.clone(),
        )?;
        std::mem::forget(handle);

        let queue_el = container
            .query_selector(".vc-queue")?
            .ok_or_else(|| JsValue::from_str("queue placeholder missing"))?;
        let queue_for_render = queue.clone();
        let batch_for_render = batch.clone();
        let handle = tpt_appfront_dom::render(
            &queue_el,
            Rc::new(move || queue_view(queue_for_render.get(), batch_for_render.get())),
            dispatch.clone(),
        )?;
        std::mem::forget(handle);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shell tree
// ---------------------------------------------------------------------------

/// The mounted-once shell: header, source card, preset card, actions,
/// result placeholder, (pro) preset manager + batch cards, upsell, footer.
fn shell_tree() -> UITree<Msg> {
    UITree::container(|c| {
        c.container(|header| {
            styled_text(header, "VC", "vc-brand");
            header.container(|group| {
                group.container(|row| {
                    row.heading(2, "Video Compressor");
                    #[cfg(feature = "pro")]
                    styled_text(row, "Pro", "vc-chip");
                })
                .class("vc-title-row");
                styled_text(
                    group,
                    "Compress and convert video locally — nothing is uploaded, there is no server.",
                    "vc-sub",
                );
            })
            .class("vc-title-group");
        })
        .class("vc-header");

        c.container(|shell| {
            shell.container(|sidebar| {
                // 1 · Source
                sidebar.container(|card| {
                    card.heading(3, "Source clip");
                    // The drop zone div is appended here by mount_app.
                    card.container(|_| {}).class("vc-dropzone-holder");
                    // The original clip's <video> preview lands here.
                    card.container(|_| {}).class("vc-source-preview");
                    // Reactive file meta / limits.
                    card.container(|_| {}).class("vc-source-meta");
                })
                .class("vc-card");

                // 2 · Export preset
                sidebar.container(|card| {
                    card.heading(3, "Export preset");
                    card.container(|row| {
                        row.container(|field| {
                            styled_text(field, "Platform quick-fill", "vc-label");
                            field
                                .select(
                                    std::iter::once(
                                        ("custom".to_string(), "Custom (fields below)".to_string()),
                                    )
                                    .chain(
                                        ExportPreset::platform_presets()
                                            .into_iter()
                                            .map(|(label, _)| (label.to_string(), label.to_string())),
                                    ),
                                    "custom",
                                )
                                .class("vc-input vc-platform")
                                .on_input(|value| Msg::PlatformChosen(value));
                        })
                        .class("vc-field");
                                                // Format: MP4/H.264 only. The core's libvpx-vp9
                        // traps ("memory access out of bounds") on the
                        // current @ffmpeg/core build, so WebM is not offered
                        // from the UI (the engine keeps the code path for a
                        // future core bump).
                    })
                    .class("vc-row");
                    card.container(|row| {
                        row.container(|field| {
                            styled_text(field, "Max width", "vc-label");
                            field
                                .select(
                                    [
                                        ("0", "Original"),
                                        ("1920", "1920 (Full HD)"),
                                        ("1280", "1280"),
                                        ("1080", "1080"),
                                        ("960", "960"),
                                        ("640", "640"),
                                    ],
                                    "1080",
                                )
                                .class("vc-input vc-width");
                        })
                        .class("vc-field");
                        row.container(|field| {
                            styled_text(field, "Frame rate", "vc-label");
                            field
                                .select([("0", "Original"), ("30", "30"), ("24", "24")], "0")
                                .class("vc-input vc-fps");
                        })
                        .class("vc-field");
                    })
                    .class("vc-row");
                    card.container(|row| {
                        row.container(|field| {
                            styled_text(field, "Quality", "vc-label");
                            field
                                .select(
                                    [
                                        ("high", "High (larger)"),
                                        ("balanced", "Balanced"),
                                        ("small", "Small (smaller)"),
                                    ],
                                    "balanced",
                                )
                                .class("vc-input vc-quality");
                        })
                        .class("vc-field");
                        row.container(|field| {
                            styled_text(field, "Audio", "vc-label");
                            field
                                .select(
                                    [
                                        ("128", "128 kbps"),
                                        ("96", "96 kbps"),
                                        ("64", "64 kbps"),
                                        ("none", "No audio"),
                                    ],
                                    "96",
                                )
                                .class("vc-input vc-audio");
                        })
                        .class("vc-field");
                    })
                    .class("vc-row");
                    styled_text(
                        card,
                        "Quality is quality-targeted (CRF), so the same preset adapts to the footage — a talking head compresses smaller than sports.",
                        "vc-hint",
                    );

                    // (pro) Branding overlay
                    #[cfg(feature = "pro")]
                    {
                        card.container(|row| {
                            row.container(|field| {
                                styled_text(field, "Branding overlay (PNG)", "vc-label");
                                field.container(|actions| {
                                    actions
                                        .button("Choose PNG\u{2026}")
                                        .class("vc-btn")
                                        .on_click(Msg::PickWatermark);
                                    actions.container(|_| {}).class("vc-wm-file");
                                })
                                .class("vc-actions");
                            })
                            .class("vc-field");
                            row.container(|field| {
                                styled_text(field, "Corner", "vc-label");
                                field
                                    .select(
                                        [
                                            ("tl", "Top left"),
                                            ("tr", "Top right"),
                                            ("bl", "Bottom left"),
                                            ("br", "Bottom right"),
                                        ],
                                        "br",
                                    )
                                    .class("vc-input vc-wm-corner");
                            })
                            .class("vc-field");
                        })
                        .class("vc-row");
                        card.container(|row| {
                            row.container(|field| {
                                styled_text(field, "Overlay width (% of video)", "vc-label");
                                field.input("15").class("vc-input vc-wm-size");
                            })
                            .class("vc-field");
                            row.container(|field| {
                                styled_text(field, "Margin (px)", "vc-label");
                                field.input("16").class("vc-input vc-wm-margin");
                            })
                            .class("vc-field");
                        })
                        .class("vc-row");
                    }
                })
                .class("vc-card");

                // (pro) 3 · Saved presets (per client)
                #[cfg(feature = "pro")]
                sidebar.container(|card| {
                    card.heading(3, "Saved presets");
                    card.container(|_| {}).class("vc-presets");
                })
                .class("vc-card");

                // Compress actions
                sidebar.container(|card| {
                    card.container(|actions| {
                        actions
                            .button(
                                #[cfg(feature = "pro")]
                                "Compress",
                                #[cfg(not(feature = "pro"))]
                                "Compress clip",
                            )
                            .class("vc-btn vc-primary vc-compress")
                            .on_click(Msg::Compress);
                    })
                    .class("vc-actions");
                    // Progress bar placeholder — raw nodes appended by mount_app.
                    card.container(|_| {}).class("vc-progress");
                })
                .class("vc-card");
            })
            .class("vc-sidebar");

            shell.container(|main| {
                // Result
                main.container(|card| {
                    card.heading(3, "Result");
                    // Raw <video> for the output, managed by publish_output.
                    card.container(|_| {}).class("vc-output-preview");
                    // Reactive result details.
                    card.container(|_| {}).class("vc-results");
                })
                .class("vc-card");

                // (pro) Batch
                #[cfg(feature = "pro")]
                main.container(|card| {
                    card.heading(3, "Batch folder");
                    styled_text(
                        card,
                        "Queue a whole folder (or a multi-file selection) and compress every clip with the current preset, one after another.",
                        "vc-hint",
                    );
                    card.container(|actions| {
                        actions
                            .button("Add files\u{2026}")
                            .class("vc-btn")
                            .on_click(Msg::BatchPickFiles);
                        actions
                            .button("Add folder\u{2026}")
                            .class("vc-btn")
                            .on_click(Msg::BatchPickFolder);
                        actions
                            .button("Compress queue")
                            .class("vc-btn vc-primary")
                            .on_click(Msg::BatchStart);
                    })
                    .class("vc-actions");
                    card.container(|_| {}).class("vc-queue");
                })
                .class("vc-card");

                // (free) Upsell
                #[cfg(not(feature = "pro"))]
                main.container(|upsell| {
                    upsell.heading(3, "Compressing for several clients?");
                    upsell.text(
                        "The Pro edition ($19, desktop) adds unlimited saved per-client presets, \
                         batch folder processing, branding overlays, and no clip length or size \
                         limits — all offline in a Windows app.",
                    );
                    upsell
                        .link("https://tptsolutions.co.nz/tools/video-compressor", "Get the Pro edition");
                })
                .class("vc-upsell");
            })
            .class("vc-main");
        })
        .class("vc-shell");

        // Present in both editions — the Pro desktop shell has no browser
        // chrome and no other route back to tptsolutions.co.nz once
        // installed. See `visit_site`/`footer_link` for why these are
        // buttons, not anchors.
        c.container(|footer| {
            styled_text(footer, "TPT Solutions", "vc-footer-brand");
            footer.container(|links| {
                footer_link(links, "More tools", "https://tptsolutions.co.nz/tools");
                footer_link(links, "tptsolutions.co.nz", "https://tptsolutions.co.nz");
            })
            .class("vc-footer-links");
        })
        .class("vc-footer");
    })
}

/// A footer "link" rendered as a plain `<button>` (not `<a>`) dispatching
/// [`Msg::VisitSite`] — the DOM reconciler's click handler never calls
/// `preventDefault`, so an anchor with both an href and a click handler
/// would navigate away from the app.
fn footer_link(c: &mut tpt_appfront_core::ContainerBuilder<Msg>, label: &str, url: &str) {
    c.button(label)
        .class("vc-footer-link")
        .on_click(Msg::VisitSite(url.to_string()));
}

/// Same fix as fea-lite's: `.text()` produces a bare DOM text node that
/// can't carry a class — wrap styled text in its own container.
fn styled_text(
    c: &mut tpt_appfront_core::ContainerBuilder<Msg>,
    text: impl Into<String>,
    class: &str,
) {
    let text = text.into();
    c.container(move |c| {
        c.text(text);
    })
    .class(class);
}

// ---------------------------------------------------------------------------
// Reactive sections
// ---------------------------------------------------------------------------

fn source_meta_view(source: Option<Rc<SourceClip>>, job: Job) -> UITree<Msg> {
    UITree::container(|c| match (&source, &job) {
        (None, _) => {
            styled_text(c, "Pick or drop a clip to begin — MP4, MOV, MKV, WebM and more.", "vc-hint");
        }
        (Some(clip), Job::Failed(msg)) => {
            styled_text(c, format!("{} · {}", clip.file_name, human_size(clip.size_bytes)), "vc-meta");
            styled_text(c, msg.clone(), "vc-error");
        }
        #[cfg(not(feature = "pro"))]
        (Some(clip), Job::LimitHit(msg)) => {
            styled_text(c, format!("{} · {}", clip.file_name, human_size(clip.size_bytes)), "vc-meta");
            c.container(|limit| {
                styled_text(limit, msg.clone(), "vc-limit-text");
            })
            .class("vc-limit");
        }
        (Some(clip), _) => {
            let mut meta = format!("{} · {}", clip.file_name, human_size(clip.size_bytes));
            if let Some(info) = &clip.info {
                meta.push_str(&format!(
                    " · {} · {} × {} · {:.1} fps",
                    info.video_codec, info.width, info.height, info.fps
                ));
                if info.duration_secs > 0.0 {
                    meta.push_str(&format!(" · {}", human_duration(info.duration_secs)));
                }
                if let Some(audio) = &info.audio {
                    meta.push_str(&format!(" · {} {}", audio.codec, audio.channels));
                }
            }
            styled_text(c, meta, "vc-meta");
        }
    })
}

fn results_view(job: Job) -> UITree<Msg> {
    UITree::container(|c| match &job {
        Job::Idle => {
            styled_text(c, "The compressed copy will appear here.", "vc-hint");
        }
        Job::Working(what) => {
            styled_text(c, what.clone(), "vc-working");
        }
        Job::Encoding(_) => {
            styled_text(c, "Encoding\u{2026} keep this tab open.", "vc-working");
        }
        Job::Failed(msg) => {
            styled_text(c, msg.clone(), "vc-error");
        }
        #[cfg(not(feature = "pro"))]
        Job::LimitHit(msg) => {
            styled_text(c, msg.clone(), "vc-error");
        }
        Job::Done(outcome) => {
            let saved_pct = if outcome.input_size > 0 {
                100.0 - (outcome.output_size as f64 / outcome.input_size as f64 * 100.0)
            } else {
                0.0
            };
            c.container(|summary| {
                styled_text(
                    summary,
                    format!(
                        "{} — {} (was {}) · {:.0}% smaller",
                        outcome.output_name,
                        human_size(outcome.output_size),
                        human_size(outcome.input_size),
                        saved_pct.max(0.0)
                    ),
                    "vc-headline",
                );
                styled_text(
                    summary,
                    format!(
                        "{} · plays in the preview above; download it to keep it.",
                        outcome.container_label
                    ),
                    "vc-detail",
                );
            })
            .class("vc-summary");
            c.container(|actions| {
                actions
                    .button(format!("Download {}", outcome.output_name))
                    .class("vc-btn vc-primary")
                    .on_click(Msg::DownloadResult);
            })
            .class("vc-actions");
        }
    })
}

/// (pro) The saved-preset manager rows.
#[cfg(feature = "pro")]
fn presets_view(presets: Vec<ExportPreset>) -> UITree<Msg> {
    UITree::container(|c| {
        c.container(|row| {
            row.input("")
                .class("vc-input vc-preset-name")
                .attr("placeholder", "Client / preset name\u{2026}");
            row.button("Save current").class("vc-btn").on_click(Msg::SavePreset);
        })
        .class("vc-row vc-preset-row");
        if presets.is_empty() {
            styled_text(
                c,
                "No saved presets yet — set the fields above, type a client name, and save. They persist in this app.",
                "vc-hint",
            );
        } else {
            for (index, preset) in presets.iter().enumerate() {
                c.container(|row| {
                    styled_text(
                        row,
                        format!(
                            "{} · {} · {}{}",
                            preset.name,
                            match preset.container {
                                Container::Mp4 => "MP4",
                                Container::WebM => "WebM",
                            },
                            if preset.max_width == 0 {
                                "original width".to_string()
                            } else {
                                format!("max {}", preset.max_width)
                            },
                            if preset.watermark.is_some() { " · overlay" } else { "" },
                        ),
                        "vc-preset-label",
                    );
                    row.button("Apply").class("vc-btn").on_click(Msg::ApplyPreset(index));
                    row.button("Delete").class("vc-btn").on_click(Msg::DeletePreset(index));
                })
                .class("vc-row vc-preset-row");
            }
        }
    })
}

// ---------------------------------------------------------------------------
// (pro) Batch queue model
// ---------------------------------------------------------------------------

/// (pro) One batch queue row.
#[cfg(feature = "pro")]
#[derive(Clone)]
struct QueueItem {
    name: String,
    size_bytes: u64,
    status: QueueStatus,
    /// Index into the BATCH_OUTPUTS slot once done (drives its download
    /// button); `None` before.
    output_slot: Option<usize>,
}

#[cfg(feature = "pro")]
#[derive(Clone, PartialEq)]
enum QueueStatus {
    Pending,
    Working,
    Done,
    Failed(String),
}

#[cfg(feature = "pro")]
#[derive(Clone, PartialEq)]
enum BatchState {
    Idle,
    Running { total: usize, finished: usize },
}

#[cfg(feature = "pro")]
fn queue_view(queue: Vec<QueueItem>, batch: BatchState) -> UITree<Msg> {
    UITree::container(|c| {
        if queue.is_empty() {
            styled_text(
                c,
                "No clips queued yet. The batch uses the export preset currently on the left.",
                "vc-hint",
            );
            return;
        }
        if let BatchState::Running { total, finished } = &batch {
            styled_text(c, format!("Running\u{2026} {finished}/{total} clips done"), "vc-working");
        }
        for (index, item) in queue.iter().enumerate() {
            c.container(|row| {
                styled_text(
                    row,
                    format!("{} · {}", item.name, human_size(item.size_bytes)),
                    "vc-queue-name",
                );
                match &item.status {
                    QueueStatus::Pending => styled_text(row, "pending", "vc-queue-status"),
                    QueueStatus::Working => styled_text(row, "working\u{2026}", "vc-queue-status"),
                    QueueStatus::Done => styled_text(row, "done", "vc-queue-status vc-queue-done"),
                    QueueStatus::Failed(err) => {
                        styled_text(row, format!("failed: {err}"), "vc-queue-status vc-queue-failed")
                    }
                }
                if let Some(slot) = item.output_slot {
                    row.button("Download")
                        .class("vc-btn")
                        .on_click(Msg::DownloadBatch(slot));
                }
                let _ = index;
            })
            .class("vc-row vc-queue-row");
        }
    })
}

// ---------------------------------------------------------------------------
// Raw-DOM wiring (drop zone, previews, progress, file inputs)
// ---------------------------------------------------------------------------

fn hidden_input(
    document: &web_sys::Document,
    container: &web_sys::Element,
    accept: &str,
    multiple: bool,
) -> Result<web_sys::HtmlInputElement, JsValue> {
    let el: web_sys::HtmlInputElement = document
        .create_element("input")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("input cast failed"))?;
    el.set_attribute("type", "file")?;
    if !accept.is_empty() {
        el.set_attribute("accept", accept)?;
    }
    if multiple {
        el.set_attribute("multiple", "")?;
    }
    el.set_attribute("style", "display:none")?;
    container.append_child(&el)?;
    Ok(el)
}

/// Builds the drop zone div (click = picker, drag & drop) and appends it
/// into the shell's `.vc-dropzone-holder` placeholder.
fn wire_drop_zone(
    container: &web_sys::Element,
    file_input: &web_sys::HtmlInputElement,
) -> Result<(), JsValue> {
    let document = web_sys::window().unwrap().document().unwrap();
    let holder = container
        .query_selector(".vc-dropzone-holder")?
        .ok_or_else(|| JsValue::from_str("dropzone holder missing"))?;

    let zone: web_sys::HtmlElement = document
        .create_element("div")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("div cast failed"))?;
    zone.set_attribute("class", "vc-dropzone")?;
    zone.set_text_content(Some("Drop a video here, or click to browse"));
    holder.append_child(&zone)?;

    {
        let file_input = file_input.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            let _ = file_input.click();
        });
        zone.add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }
    {
        let zone_over = zone.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.prevent_default();
            let _ = zone_over.set_attribute("class", "vc-dropzone vc-dropzone-over");
        });
        zone.add_event_listener_with_callback("dragover", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }
    {
        let zone_leave = zone.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.prevent_default();
            let _ = zone_leave.set_attribute("class", "vc-dropzone");
        });
        zone.add_event_listener_with_callback("dragleave", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }
    {
        let zone_drop = zone.clone();
        let source_wire: Closure<dyn FnMut(web_sys::DragEvent)> =
            Closure::new(move |event: web_sys::DragEvent| {
                event.prevent_default();
                let _ = zone_drop.set_attribute("class", "vc-dropzone");
                if let Some(data) = event.data_transfer() {
                    if let Some(file) = data.files().and_then(|f| f.get(0)) {
                        set_pending_drop(file);
                    }
                }
                if let Some(cb) = PENDING_DROP_CB.with(|slot| slot.borrow().clone()) {
                    cb();
                }
            });
        zone.add_event_listener_with_callback("drop", source_wire.as_ref().unchecked_ref())?;
        source_wire.forget();
    }
    Ok(())
}

thread_local! {
    /// A file dropped on the zone, waiting for the source-flow callback.
    static PENDING_DROP: RefCell<Option<web_sys::File>> = const { RefCell::new(None) };
    /// The source-flow entry point, registered by `wire_source_input`.
    static PENDING_DROP_CB: RefCell<Option<Rc<dyn Fn()>>> = const { RefCell::new(None) };
}

fn set_pending_drop(file: web_sys::File) {
    PENDING_DROP.with(|slot| *slot.borrow_mut() = Some(file));
}

/// Wires the hidden source input's change listener AND registers the
/// drop-zone entry point that kicks off the same flow.
fn wire_source_input(
    input: &web_sys::HtmlInputElement,
    source: &Signal<Option<Rc<SourceClip>>>,
    job: &Signal<Job>,
) {
    // Change-listener flow: files picked via the hidden input.
    {
        let input_for_change = input.clone();
        let source = source.clone();
        let job = job.clone();
        let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            if let Some(files) = input_for_change.files() {
                if let Some(file) = files.get(0) {
                    start_source_flow(&file, &source, &job);
                }
            }
        });
        input
            .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
            .ok();
        closure.forget();
    }

    // Drop-zone flow: the zone's drop handler stashes the file in
    // PENDING_DROP and calls this registered kicker.
    {
        let source = source.clone();
        let job = job.clone();
        PENDING_DROP_CB.with(|slot| {
            *slot.borrow_mut() = Some(Rc::new(move || {
                let file = PENDING_DROP.with(|slot| slot.borrow_mut().take());
                if let Some(file) = file {
                    start_source_flow(&file, &source, &job);
                }
            }));
        });
    }
}

/// Reads the picked file, previews it, loads the engine, probes it, and
/// publishes the source (or the free-tier limit message).
fn start_source_flow(file: &web_sys::File, source: &Signal<Option<Rc<SourceClip>>>, job: &Signal<Job>) {
    let file_name = file.name();
    let size_bytes = file.size() as u64;

    // Free edition: size limit is checkable before any engine work.
    #[cfg(not(feature = "pro"))]
    if size_bytes > FREE_MAX_INPUT_BYTES {
        job.set(Job::LimitHit(format!(
            "{} is {} — over the free edition's {} limit. The Pro edition has no size limit.",
            file_name,
            human_size(size_bytes),
            human_size(FREE_MAX_INPUT_BYTES)
        )));
        return;
    }

    let preview_url = {
        let blob_ref: &web_sys::Blob = file.as_ref();
        web_sys::Url::create_object_url_with_blob(blob_ref).unwrap_or_default()
    };

    job.set(Job::Working("Reading clip\u{2026}".to_string()));
    source.set(Some(Rc::new(SourceClip {
        file_name,
        size_bytes,
        preview_url,
        info: None,
    })));

    let source = source.clone();
    let job = job.clone();
    let file = file.clone();
    wasm_bindgen_futures::spawn_local(async move {
        match load_engine().await {
            Ok(engine) => {
                job.set(Job::Working("Probing clip\u{2026}".to_string()));
                match probe_fs_clip(&engine, &file).await {
                    Ok(info) => {
                        if let Some(mut clip) = source.get().map(|c| (*c).clone()) {
                            // Free edition: duration limit.
                            #[cfg(not(feature = "pro"))]
                            if info.duration_secs > FREE_MAX_DURATION_SECS {
                                job.set(Job::LimitHit(format!(
                                    "{} is {} long — over the free edition's 5-minute limit. \
                                     The Pro edition has no length limit.",
                                    clip.file_name,
                                    human_duration(info.duration_secs)
                                )));
                                return;
                            }
                            clip.info = Some(Rc::new(info));
                            source.set(Some(Rc::new(clip)));
                            job.set(Job::Idle);
                        }
                    }
                    Err(err) => job.set(Job::Failed(format!("Could not read this file as video: {err}"))),
                }
            }
            Err(err) => job.set(Job::Failed(format!(
                "Could not load the compression engine: {err}"
            ))),
        }
    });
}

/// The output <video> preview: `holder` is emptied or filled with a
/// <video controls> for `media`.
fn update_video_preview(holder: &web_sys::Element, media: Option<(String, String)>) -> Result<(), JsValue> {
    let document = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    holder.set_inner_html("");
    let Some((url, _name)) = media else {
        return Ok(());
    };
    let video: web_sys::HtmlVideoElement = document
        .create_element("video")?
        .dyn_into()
        .map_err(|_| JsValue::from_str("video cast failed"))?;
    video.set_attribute("controls", "")?;
    video.set_attribute("src", &url)?;
    video.set_attribute("class", "vc-preview-video")?;
    holder.append_child(&video)?;
    Ok(())
}

/// Keeps the progress bar's fill width in sync (raw nodes: the DOM backend
/// has no progress-bar node kind).
fn update_progress_bar(container: &web_sys::Element, ratio: Option<f64>) -> Result<(), JsValue> {
    let Some(holder) = container.query_selector(".vc-progress").ok().flatten() else {
        return Ok(());
    };
    let document = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let has_bar = holder.query_selector(".vc-progress-fill")?.is_some();
    if !has_bar {
        if ratio.is_none() {
            return Ok(());
        }
        let track: web_sys::HtmlElement = document
            .create_element("div")?
            .dyn_into()
            .map_err(|_| JsValue::from_str("div cast failed"))?;
        track.set_attribute("class", "vc-progress-track")?;
        let fill: web_sys::HtmlElement = document
            .create_element("div")?
            .dyn_into()
            .map_err(|_| JsValue::from_str("div cast failed"))?;
        fill.set_attribute("class", "vc-progress-fill")?;
        fill.set_attribute("style", "width:0%")?;
        track.append_child(&fill)?;
        holder.append_child(&track)?;
    }
    if let Some(ratio) = ratio {
        if let Some(fill) = holder.query_selector(".vc-progress-fill")? {
            let pct = (ratio.clamp(0.0, 1.0) * 100.0).min(99.0);
            fill.set_attribute("style", &format!("width:{pct:.1}%"))?;
        }
    } else if let Some(track) = holder.query_selector(".vc-progress-track")? {
        let _ = track.remove();
    }
    Ok(())
}

fn set_compress_busy(container: &web_sys::Element, busy: bool) -> Result<(), JsValue> {
    if let Some(btn) = container
        .query_selector(".vc-compress")?
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
    {
        if busy {
            btn.set_attribute("disabled", "")?;
        } else {
            let _ = btn.remove_attribute("disabled");
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Engine session + probe + compress
// ---------------------------------------------------------------------------

thread_local! {
    /// The one ffmpeg worker connection for the app's lifetime.
    static ENGINE: RefCell<Option<Rc<FFmpeg>>> = const { RefCell::new(None) };
    /// Where `ffmpeg.rs`'s log callback currently delivers stderr lines.
    static LOG_SINK: RefCell<Option<Rc<RefCell<String>>>> = const { RefCell::new(None) };
    /// Where the progress callback currently delivers ratios.
    static PROGRESS_SINK: RefCell<Option<Box<dyn Fn(f64, f64)>>> = const { RefCell::new(None) };
    /// (pro) The watermark PNG bytes picked via the hidden input.
    #[cfg(feature = "pro")]
    static WATERMARK_BYTES: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// (pro) Batch items in enqueue order, with their bytes.
    #[cfg(feature = "pro")]
    static BATCH_FILES: RefCell<Vec<(String, u64, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    /// (pro) Batch outputs in completion order.
    #[cfg(feature = "pro")]
    static BATCH_OUTPUTS: RefCell<Vec<Rc<JobOutcome>>> = const { RefCell::new(Vec::new()) };
    /// The most recent single-clip result (drives the Result download).
    static LAST_OUTPUT: RefCell<Option<Rc<JobOutcome>>> = const { RefCell::new(None) };
}

#[cfg(feature = "pro")]
fn set_watermark_bytes(bytes: Vec<u8>) {
    WATERMARK_BYTES.with(|slot| *slot.borrow_mut() = Some(bytes));
}

#[cfg(feature = "pro")]
fn watermark_bytes() -> Option<Vec<u8>> {
    WATERMARK_BYTES.with(|slot| slot.borrow().clone())
}

/// Loads the engine once; later calls reuse the connection. The log and
/// progress callbacks are installed here, once, and route through the
/// module-wide sinks.
async fn load_engine() -> Result<Rc<FFmpeg>, String> {
    if let Some(engine) = ENGINE.with(|slot| slot.borrow().clone()) {
        return Ok(engine);
    }
    let log_sink = LOG_SINK.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Rc::new(RefCell::new(String::new())));
        }
        slot.as_ref().map(Rc::clone)
    });
    let engine = FFmpeg::load(
        Rc::new(move |kind, message| {
            // Only stderr is captured (the probe parser reads it); other
            // stream tags go to the devtools console for debugging.
            if kind == "stderr" {
                if let Some(sink) = &log_sink {
                    let mut text = sink.borrow_mut();
                    text.push_str(message);
                    text.push('\n');
                }
            } else {
                console_log(&format!("ffmpeg[{kind}] {message}"));
            }
        }),
        Rc::new(move |ratio, time| {
            PROGRESS_SINK.with(|slot| {
                if let Some(cb) = slot.borrow().as_ref() {
                    cb(ratio, time);
                }
            });
        }),
    )
    .await
    .map_err(|err| format!("engine load failed: {}", js_error_text(&err)))?;
    let engine = Rc::new(engine);
    ENGINE.with(|slot| *slot.borrow_mut() = Some(engine.clone()));
    Ok(engine)
}

/// Reads the clip, stages it in the worker FS, runs `ffmpeg -i`, and
/// parses the stderr the logger delivered. The exit code is 1 by design
/// (no output requested) and is ignored — the worker survives failed runs.
async fn probe_fs_clip(engine: &FFmpeg, file: &web_sys::File) -> Result<MediaInfo, String> {
    let bytes = read_file_bytes(file).await.map_err(|e| js_error_text(&e))?;
    engine
        .write_file(INPUT_NAME, &bytes)
        .await
        .map_err(|e| js_error_text(&e))?;

    LOG_SINK.with(|slot| {
        if let Some(sink) = slot.borrow().as_ref() {
            sink.borrow_mut().clear();
        }
    });
    let _ = engine.exec(&["-hide_banner", "-i", INPUT_NAME]).await;

    let log = LOG_SINK.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|sink| sink.borrow().clone())
            .unwrap_or_default()
    });
    parse_probe_log(&log).ok_or_else(|| "no video stream found (is this really a video file?)".to_string())
}

/// The main compression run: reads the form, builds the command, encodes,
/// reads the output back, and publishes the Done outcome.
fn run_compress(source: &Signal<Option<Rc<SourceClip>>>, job: &Signal<Job>) {
    let Some(clip) = source.get() else {
        job.set(Job::Failed("Pick a clip first.".to_string()));
        return;
    };

    let preset = match read_preset_from_form() {
        Ok(preset) => preset,
        Err(msg) => {
            job.set(Job::Failed(msg));
            return;
        }
    };

    let job = job.clone();
    let clip = (*clip).clone();
    wasm_bindgen_futures::spawn_local(async move {
        job_coalesce(&job, Job::Working("Loading engine\u{2026}".to_string()));
        let engine = match load_engine().await {
            Ok(engine) => engine,
            Err(err) => {
                job.set(Job::Failed(err));
                return;
            }
        };

        let source_width = clip.info.as_ref().map(|i| i.width).unwrap_or(0);
        let wm_width = preset
            .watermark
            .as_ref()
            .map(|_| cmd::resolve_watermark_width(&preset, source_width.max(1)));

        #[cfg(feature = "pro")]
        if preset.watermark.is_some() {
            match watermark_bytes() {
                Some(bytes) => {
                    if let Err(err) = engine.write_file(WATERMARK_FS_NAME, &bytes).await {
                        job.set(Job::Failed(format!(
                            "Could not stage the watermark: {}",
                            js_error_text(&err)
                        )));
                        return;
                    }
                }
                None => {
                    job.set(Job::Failed("Pick a PNG for the branding overlay first.".to_string()));
                    return;
                }
            }
        }

        // Progress: pipe core ratios into the Encoding state (coalesced to
        // whole percent to keep re-renders cheap).
        {
            let job = job.clone();
            PROGRESS_SINK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move |ratio, _time| {
                    job_coalesce(&job, Job::Encoding(ratio.clamp(0.0, 1.0)));
                }));
            });
        }

        let output_name = output_file_name(&clip.file_name, preset.extension());
        let output_fs_name = format!("output.{}", preset.extension());
        let args = cmd::build_ffmpeg_args(&preset, INPUT_NAME, &output_fs_name, wm_width);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let exit = match engine.exec(&arg_refs).await {
            Ok(code) => code,
            Err(err) => {
                PROGRESS_SINK.with(|slot| *slot.borrow_mut() = None);
                job.set(Job::Failed(format!("Engine error while encoding: {}", js_error_text(&err))));
                return;
            }
        };
        PROGRESS_SINK.with(|slot| *slot.borrow_mut() = None);

        if exit != 0 {
            job.set(Job::Failed(format!(
                "ffmpeg failed (exit {exit}) — try different settings or a lower quality."
            )));
            return;
        }

        job_coalesce(&job, Job::Working("Reading result\u{2026}".to_string()));
        match engine.read_file(&output_fs_name).await {
            Ok(data) => {
                engine.delete_file(&output_fs_name);
                let outcome = publish_output(&clip, &output_name, &data, &preset);
                LAST_OUTPUT.with(|slot| *slot.borrow_mut() = Some(outcome.clone()));
                job.set(Job::Done(outcome));
            }
            Err(err) => {
                job.set(Job::Failed(format!(
                    "ffmpeg reported success but the output could not be read: {}",
                    js_error_text(&err)
                )));
            }
        }
    });
}

/// Builds the blob URL for `data`, wires it into the output preview, and
/// returns the outcome. (Split out so the batch runner can reuse it.)
fn publish_output(clip: &SourceClip, output_name: &str, data: &[u8], preset: &ExportPreset) -> Rc<JobOutcome> {
    let mime = match preset.container {
        Container::Mp4 => "video/mp4",
        Container::WebM => "video/webm",
    };
    let array = js_sys::Array::new();
    array.push(&js_sys::Uint8Array::from(data));
    let bag = web_sys::BlobPropertyBag::new();
    bag.set_type(mime);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&array, &bag).ok();
    let url = blob
        .and_then(|b| web_sys::Url::create_object_url_with_blob(&b).ok())
        .unwrap_or_default();

    let outcome = Rc::new(JobOutcome {
        output_url: url.clone(),
        output_name: output_name.to_string(),
        output_size: data.len() as u64,
        input_size: clip.size_bytes,
        container_label: match preset.container {
            Container::Mp4 => "MP4 / H.264",
            Container::WebM => "WebM / VP9",
        },
    });

    // Wire the output preview directly (the reactive section can't carry
    // the raw <video>).
    if let Some(container) = current_container() {
        if let Ok(Some(holder)) = container.query_selector(".vc-output-preview") {
            let _ = update_video_preview(&holder, Some((url, output_name.to_string())));
        }
    }
    outcome
}

// ---------------------------------------------------------------------------
// (pro) Preset persistence + batch runner
// ---------------------------------------------------------------------------

#[cfg(feature = "pro")]
const PRESETS_KEY: &str = "tpt-vc-presets-v1";

#[cfg(feature = "pro")]
fn load_saved_presets() -> Vec<ExportPreset> {
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(PRESETS_KEY).ok().flatten())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

#[cfg(feature = "pro")]
fn persist_presets(presets: &[ExportPreset]) {
    if let Ok(json) = serde_json::to_string(presets) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item(PRESETS_KEY, &json);
        }
    }
}

#[cfg(feature = "pro")]
fn save_preset(container: &web_sys::Element, presets: &Signal<Vec<ExportPreset>>) {
    let name = read_input(container, ".vc-preset-name").trim().to_string();
    if name.is_empty() {
        console_log("preset name required");
        return;
    }
    let mut preset = read_preset_from_form().unwrap_or_default();
    preset.name = name;
    let mut list = presets.get();
    list.retain(|p| p.name != preset.name);
    list.push(preset);
    presets.set(list);
    persist_presets(&presets.get());
}

#[cfg(feature = "pro")]
fn apply_saved_preset(container: &web_sys::Element, presets: &Signal<Vec<ExportPreset>>, index: usize) {
    let list = presets.get();
    let Some(preset) = list.get(index) else { return };
    let preset = preset.clone();
    write_select(container, ".vc-format", container_value(preset.container));
    write_select(container, ".vc-width", &preset.max_width.to_string());
    write_select(container, ".vc-fps", &preset.fps.to_string());
    write_select(container, ".vc-quality", quality_value(preset.quality));
    write_select(container, ".vc-audio", audio_value(preset.audio));
}

#[cfg(feature = "pro")]
fn delete_saved_preset(presets: &Signal<Vec<ExportPreset>>, index: usize) {
    let mut list = presets.get();
    if index < list.len() {
        list.remove(index);
        presets.set(list);
        persist_presets(&presets.get());
    }
}

fn container_value(c: Container) -> &'static str {
    match c {
        Container::Mp4 => "mp4",
        Container::WebM => "webm",
    }
}

fn quality_value(q: Quality) -> &'static str {
    match q {
        Quality::High => "high",
        Quality::Balanced => "balanced",
        Quality::Small => "small",
    }
}

fn audio_value(a: Audio) -> &'static str {
    match a {
        Audio::Aac128 => "128",
        Audio::Aac96 => "96",
        Audio::Aac64 => "64",
        Audio::None => "none",
    }
}

/// (pro) Wires the watermark PNG input: reads the bytes once at pick time
/// and shows the chosen filename.
#[cfg(feature = "pro")]
fn wire_watermark_input(input: &web_sys::HtmlInputElement) {
    let input_for_change = input.clone();
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        let Some(files) = input_for_change.files() else { return };
        let Some(file) = files.get(0) else { return };
        let file = file.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let name = file.name();
            match read_file_bytes(&file).await {
                Ok(bytes) => {
                    set_watermark_bytes(bytes);
                    if let Some(container) = current_container() {
                        if let Ok(Some(label)) = container.query_selector(".vc-wm-file") {
                            let _ = label.set_text_content(Some(&name));
                        }
                    }
                }
                Err(err) => console_log(&format!("watermark read failed: {}", js_error_text(&err))),
            }
        });
    });
    input
        .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
        .ok();
    closure.forget();
}

/// (pro) Wires a batch input's change listener: reads every picked file's
/// bytes at enqueue time (so the File handles can't go stale), appends to
/// the queue, and leaves starting to the explicit Compress-queue button.
#[cfg(feature = "pro")]
fn wire_batch_inputs(input: &web_sys::HtmlInputElement, queue: &Signal<Vec<QueueItem>>) {
    let input_for_change = input.clone();
    let queue = queue.clone();
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
        let Some(files) = input_for_change.files() else { return };
        let picked: Vec<web_sys::File> = (0..files.length()).filter_map(|i| files.get(i)).collect();
        if picked.is_empty() {
            return;
        }
        let queue = queue.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut new_items = Vec::new();
            let mut new_bytes = Vec::new();
            for file in picked {
                let name = file.name();
                let size = file.size() as u64;
                match read_file_bytes(&file).await {
                    Ok(bytes) => {
                        new_items.push(QueueItem {
                            name: name.clone(),
                            size_bytes: size,
                            status: QueueStatus::Pending,
                            output_slot: None,
                        });
                        new_bytes.push((name, size, bytes));
                    }
                    Err(_) => {
                        new_items.push(QueueItem {
                            name,
                            size_bytes: size,
                            status: QueueStatus::Failed("could not read file".to_string()),
                            output_slot: None,
                        });
                    }
                }
            }
            BATCH_FILES.with(|slot| slot.borrow_mut().extend(new_bytes));
            let mut items = queue.get();
            items.extend(new_items);
            queue.set(items);
        });
    });
    input
        .add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())
        .ok();
    closure.forget();
}

/// (pro) Runs the whole pending queue sequentially with the current form
/// preset. One engine connection; the single-threaded core serializes.
#[cfg(feature = "pro")]
fn start_batch(queue: &Signal<Vec<QueueItem>>, batch: &Signal<BatchState>, job: &Signal<Job>) {
    // Map queue rows to BATCH_FILES entries by name+size (enqueue order
    // matches).
    let pending: Vec<usize> = queue
        .get()
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item.status, QueueStatus::Pending))
        .map(|(i, _)| i)
        .collect();
    if pending.is_empty() {
        return;
    }

    let preset = match read_preset_from_form() {
        Ok(preset) => preset,
        Err(msg) => {
            job.set(Job::Failed(msg));
            return;
        }
    };

    let queue = queue.clone();
    let batch = batch.clone();
    let job = job.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let total = pending.len();
        batch.set(BatchState::Running { total, finished: 0 });
        let engine = match load_engine().await {
            Ok(engine) => engine,
            Err(err) => {
                job.set(Job::Failed(err));
                batch.set(BatchState::Idle);
                return;
            }
        };

        // Stage the watermark once, up front.
        if preset.watermark.is_some() {
            if let Some(wm_bytes) = watermark_bytes() {
                let _ = engine.write_file(WATERMARK_FS_NAME, &wm_bytes).await;
            }
        }

        for (done_so_far, queue_index) in pending.into_iter().enumerate() {
            let Some((name, size)) = queue.get().get(queue_index).map(|i| (i.name.clone(), i.size_bytes)) else {
                continue;
            };
            let bytes = BATCH_FILES.with(|slot| {
                slot.borrow()
                    .iter()
                    .find(|(n, s, _)| *n == name && *s == size)
                    .map(|(_, _, b)| b.clone())
            });
            let Some(bytes) = bytes else {
                mark_queue_item(&queue, queue_index, QueueStatus::Failed("file data unavailable".to_string()));
                continue;
            };

            mark_queue_item(&queue, queue_index, QueueStatus::Working);
            if engine.write_file(INPUT_NAME, &bytes).await.is_err() {
                mark_queue_item(&queue, queue_index, QueueStatus::Failed("could not stage file".to_string()));
                continue;
            }

            // Probe for the watermark width (stderr parse; exit code ignored).
            LOG_SINK.with(|slot| {
                if let Some(sink) = slot.borrow().as_ref() {
                    sink.borrow_mut().clear();
                }
            });
            let _ = engine.exec(&["-hide_banner", "-i", INPUT_NAME]).await;
            let log = LOG_SINK.with(|slot| {
                slot.borrow().as_ref().map(|s| s.borrow().clone()).unwrap_or_default()
            });
            let source_width = parse_probe_log(&log).map(|i| i.width).unwrap_or(1920);
            let wm_width = preset
                .watermark
                .as_ref()
                .map(|_| cmd::resolve_watermark_width(&preset, source_width.max(1)));

            let ext = preset.extension();
            let output_fs_name = format!("batch_out.{ext}");
            let args = cmd::build_ffmpeg_args(&preset, INPUT_NAME, &output_fs_name, wm_width);
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

            {
                let job = job.clone();
                PROGRESS_SINK.with(|slot| {
                    *slot.borrow_mut() = Some(Box::new(move |ratio, _time| {
                        job_coalesce(&job, Job::Encoding(ratio.clamp(0.0, 1.0)));
                    }));
                });
            }
            let exit = match engine.exec(&arg_refs).await {
                Ok(code) => code,
                Err(err) => {
                    PROGRESS_SINK.with(|slot| *slot.borrow_mut() = None);
                    mark_queue_item(&queue, queue_index, QueueStatus::Failed(js_error_text(&err)));
                    batch.set(BatchState::Running { total, finished: done_so_far + 1 });
                    continue;
                }
            };
            PROGRESS_SINK.with(|slot| *slot.borrow_mut() = None);

            if exit != 0 {
                mark_queue_item(&queue, queue_index, QueueStatus::Failed(format!("ffmpeg exit {exit}")));
            } else {
                match engine.read_file(&output_fs_name).await {
                    Ok(data) => {
                        engine.delete_file(&output_fs_name);
                        let out_name = output_file_name(&name, ext);
                        let pseudo = SourceClip {
                            file_name: name.clone(),
                            size_bytes: bytes.len() as u64,
                            preview_url: String::new(),
                            info: None,
                        };
                        let outcome = publish_output(&pseudo, &out_name, &data, &preset);
                        let slot = BATCH_OUTPUTS.with(|slot| {
                            let mut list = slot.borrow_mut();
                            list.push(outcome);
                            list.len() - 1
                        });
                        let mut items = queue.get();
                        if let Some(item) = items.get_mut(queue_index) {
                            item.status = QueueStatus::Done;
                            item.output_slot = Some(slot);
                        }
                        queue.set(items);
                    }
                    Err(err) => {
                        mark_queue_item(&queue, queue_index, QueueStatus::Failed(js_error_text(&err)));
                    }
                }
            }

            batch.set(BatchState::Running { total, finished: done_so_far + 1 });
        }
        batch.set(BatchState::Idle);
        job.set(Job::Idle);
    });
}

#[cfg(feature = "pro")]
fn mark_queue_item(queue: &Signal<Vec<QueueItem>>, index: usize, status: QueueStatus) {
    let mut items = queue.get();
    if let Some(item) = items.get_mut(index) {
        item.status = status;
    }
    queue.set(items);
}

// ---------------------------------------------------------------------------
// Form <-> preset plumbing
// ---------------------------------------------------------------------------

/// Reads the export preset straight out of the mounted form.
fn read_preset_from_form() -> Result<ExportPreset, String> {
    let Some(container) = current_container() else {
        return Err("App is still starting — try again.".to_string());
    };
    // MP4/H.264 is the only format the UI offers (see the shell note);
    // `Container::WebM` stays available to the engine and saved presets.
    let kind = Container::Mp4;
    let max_width: u16 = read_input(&container, ".vc-width").trim().parse().unwrap_or(0);
    let fps: u16 = read_input(&container, ".vc-fps").trim().parse().unwrap_or(0);
    let quality = match read_input(&container, ".vc-quality").as_str() {
        "high" => Quality::High,
        "small" => Quality::Small,
        _ => Quality::Balanced,
    };
    let audio = match read_input(&container, ".vc-audio").as_str() {
        "128" => Audio::Aac128,
        "64" => Audio::Aac64,
        "none" => Audio::None,
        _ => Audio::Aac96,
    };

    #[cfg(feature = "pro")]
    let watermark = {
        let has_png = watermark_bytes().is_some();
        if has_png {
            let size_pct: f64 = read_input(&container, ".vc-wm-size").trim().parse().unwrap_or(15.0);
            let margin: u32 = read_input(&container, ".vc-wm-margin").trim().parse().unwrap_or(16);
            let corner = match read_input(&container, ".vc-wm-corner").as_str() {
                "tl" => Corner::TopLeft,
                "tr" => Corner::TopRight,
                "bl" => Corner::BottomLeft,
                _ => Corner::BottomRight,
            };
            Some(Watermark {
                width_fraction: (size_pct / 100.0).clamp(0.02, 0.5),
                corner,
                margin_px: margin,
            })
        } else {
            None
        }
    };

    Ok(ExportPreset {
        name: String::new(),
        container: kind,
        max_width,
        fps,
        quality,
        audio,
        #[cfg(feature = "pro")]
        watermark,
        #[cfg(not(feature = "pro"))]
        watermark: None,
    })
}

/// Fills the fields from a built-in platform quick-fill.
fn apply_platform_preset(container: &web_sys::Element, label: &str) {
    if label == "custom" {
        return;
    }
    if let Some((_, preset)) = ExportPreset::platform_presets().into_iter().find(|(l, _)| *l == label) {
        write_select(container, ".vc-format", container_value(preset.container));
        write_select(container, ".vc-width", &preset.max_width.to_string());
        write_select(container, ".vc-fps", &preset.fps.to_string());
        write_select(container, ".vc-quality", quality_value(preset.quality));
        write_select(container, ".vc-audio", audio_value(preset.audio));
    }
}

fn read_input(container: &web_sys::Element, selector: &str) -> String {
    container
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|el| {
            el.dyn_into::<web_sys::HtmlInputElement>()
                .map(|input| input.value())
                .or_else(|el| {
                    el.dyn_into::<web_sys::HtmlSelectElement>()
                        .map(|select| select.value())
                })
                .ok()
        })
        .unwrap_or_default()
}

fn write_select(container: &web_sys::Element, selector: &str, value: &str) {
    if let Some(Some(select)) = container
        .query_selector(selector)
        .ok()
        .map(|el| el.and_then(|el| el.dyn_into::<web_sys::HtmlSelectElement>().ok()))
    {
        select.set_value(value);
    }
}

fn output_file_name(source_name: &str, extension: &str) -> String {
    let stem = source_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(source_name);
    format!("{stem}-compressed.{extension}")
}

// ---------------------------------------------------------------------------
// Downloads + misc
// ---------------------------------------------------------------------------

/// Clicks a temporary anchor to download a result blob.
fn trigger_download(outcome: &JobOutcome) {
    let Some(window) = web_sys::window() else { return };
    let Some(document) = window.document() else { return };
    if let Ok(anchor) = document.create_element("a") {
        let _ = anchor.set_attribute("href", &outcome.output_url);
        let _ = anchor.set_attribute("download", &outcome.output_name);
        if let Some(a) = anchor.dyn_ref::<web_sys::HtmlAnchorElement>() {
            let _ = a.click();
        }
    }
}

/// Opens a TPT Solutions URL from wherever the app happens to be running.
/// Desktop shell (app:// protocol) routes through the host's allowlisted
/// IPC bridge; browsers open a new tab.
fn visit_site(url: &str) {
    let Some(window) = web_sys::window() else { return };
    let in_desktop_shell = window
        .location()
        .protocol()
        .map(|p| p == "app:")
        .unwrap_or(false);

    if in_desktop_shell {
        if let Ok(appfront) = js_sys::Reflect::get(&window, &JsValue::from_str("__appfront")) {
            if let Ok(post) = js_sys::Reflect::get(&appfront, &JsValue::from_str("post")) {
                if let Ok(post_fn) = post.dyn_into::<js_sys::Function>() {
                    let params = js_sys::Object::new();
                    let _ = js_sys::Reflect::set(&params, &JsValue::from_str("url"), &JsValue::from_str(url));
                    let _ = post_fn.call2(&appfront, &JsValue::from_str("open_external"), &params);
                }
            }
        }
    } else {
        let _ = window.open_with_url_and_target(url, "_blank");
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Progress re-renders coalesced: only publish when the human-visible
/// state actually changes (percent granularity).
fn job_coalesce(job: &Signal<Job>, next: Job) {
    let changed = match (job.get(), &next) {
        (Job::Encoding(old), Job::Encoding(new)) => (old * 100.0) as u32 != (new * 100.0) as u32,
        (Job::Working(old), Job::Working(new)) => old != *new,
        _ => true,
    };
    if changed {
        job.set(next);
    }
}

async fn read_file_bytes(file: &web_sys::File) -> Result<Vec<u8>, JsValue> {
    let promise = file.array_buffer();
    let buffer = wasm_bindgen_futures::JsFuture::from(promise).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

fn js_error_text(err: &JsValue) -> String {
    err.as_string().unwrap_or_else(|| format!("{err:?}"))
}

fn console_log(message: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = console)]
            fn log(s: &str);
        }
        log(message);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = message;
    }
}

fn human_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 100.0 {
        format!("{mb:.0} MB")
    } else if mb >= 1.0 {
        format!("{mb:.1} MB")
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn human_duration(secs: f64) -> String {
    let total = secs.round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn upsert_style(document: &web_sys::Document, id: &str, css: &str) {
    if document.get_element_by_id(id).is_none() {
        if let Ok(Some(head)) = document.query_selector("head") {
            if let Ok(style) = document.create_element("style") {
                let _ = style.set_attribute("id", id);
                style.set_text_content(Some(css));
                let _ = head.append_child(&style);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const BASE_CSS: &str = r#"
.vc-app{--vc-bg:#f7f8fa;--vc-surface:#ffffff;--vc-panel:#f2f4f7;--vc-fg:#181d27;--vc-muted:#667085;--vc-border:#e3e7ee;--vc-accent:#4f46e5;--vc-accent-2:#4338ca;--vc-accent-soft:#eef0fe;--vc-accent-fg:#ffffff;--vc-err:#b3261e;--vc-err-bg:#fbeeec;--vc-shadow:0 1px 2px rgba(16,24,40,.04),0 1px 3px rgba(16,24,40,.06);max-width:1360px;margin:0 auto;background:var(--vc-bg);color:var(--vc-fg);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Helvetica,Arial,sans-serif;font-size:14.5px;line-height:1.55;padding:0 4px 32px}
@media(prefers-color-scheme:dark){.vc-app{--vc-bg:#0b0f16;--vc-surface:#141a24;--vc-panel:#161d2a;--vc-fg:#e8ebf0;--vc-muted:#98a2b3;--vc-border:#262f3d;--vc-accent:#818cf8;--vc-accent-2:#a5b4fc;--vc-accent-soft:#1e1b4b;--vc-accent-fg:#0b0f16;--vc-err:#f2b8b5;--vc-err-bg:#2c1a1c;--vc-shadow:0 1px 2px rgba(0,0,0,.3)}}
.vc-app *{box-sizing:border-box}
.vc-app h2{font-size:1.36rem;margin:0;font-weight:700;letter-spacing:-.01em}
.vc-app h3{font-size:1rem;margin:0 0 6px;font-weight:700}

.vc-header{display:flex;align-items:flex-start;gap:14px;padding:22px 2px 18px;border-bottom:1px solid var(--vc-border);margin-bottom:22px}
.vc-brand{flex:0 0 auto;width:42px;height:42px;border-radius:11px;background:linear-gradient(135deg,var(--vc-accent),var(--vc-accent-2));display:flex;align-items:center;justify-content:center;color:var(--vc-accent-fg);font-weight:800;font-size:.82rem;letter-spacing:.01em;box-shadow:var(--vc-shadow)}
.vc-title-group{flex:1;min-width:0}
.vc-title-row{display:flex;align-items:center;gap:10px;flex-wrap:wrap}
.vc-chip{display:inline-flex;align-items:center;padding:3px 10px;border-radius:999px;font-size:.66rem;font-weight:800;letter-spacing:.05em;text-transform:uppercase;background:var(--vc-accent-soft);color:var(--vc-accent)}
.vc-sub{color:var(--vc-muted);margin:5px 0 0;font-size:.88rem}

.vc-shell{display:grid;grid-template-columns:minmax(340px,400px) 1fr;gap:22px;align-items:start}
@media(max-width:900px){.vc-shell{grid-template-columns:1fr}}
.vc-sidebar{display:flex;flex-direction:column;gap:16px;position:sticky;top:16px;max-height:calc(100vh - 32px);overflow-y:auto;padding:2px 4px 12px 2px;counter-reset:vc-step}
@media(max-width:900px){.vc-sidebar{position:static;max-height:none;overflow:visible}}
.vc-main{display:flex;flex-direction:column;gap:16px;min-width:0}

.vc-card{background:var(--vc-surface);border:1px solid var(--vc-border);border-radius:14px;padding:16px 18px;box-shadow:var(--vc-shadow)}
.vc-sidebar .vc-card h3{display:flex;align-items:center;gap:9px}
.vc-sidebar .vc-card h3::before{counter-increment:vc-step;content:counter(vc-step);flex:0 0 auto;width:20px;height:20px;border-radius:50%;background:var(--vc-accent);color:var(--vc-accent-fg);font-size:.68rem;display:flex;align-items:center;justify-content:center;font-weight:800}

.vc-hint{color:var(--vc-muted);font-size:.8rem;margin:6px 0 0}
.vc-row{display:flex;gap:12px;flex-wrap:wrap;margin-top:10px}
.vc-field{display:flex;flex-direction:column;gap:5px;flex:1;min-width:130px}
.vc-label{font-size:.72rem;color:var(--vc-muted);font-weight:600;text-transform:uppercase;letter-spacing:.03em}
.vc-input{width:100%;padding:7px 9px;border:1px solid var(--vc-border);border-radius:8px;background:var(--vc-bg);color:var(--vc-fg);font:inherit;transition:border-color .12s,box-shadow .12s}
.vc-input:focus{outline:none;border-color:var(--vc-accent);box-shadow:0 0 0 3px var(--vc-accent-soft)}

.vc-dropzone{border:2px dashed var(--vc-border);border-radius:12px;padding:26px 14px;text-align:center;color:var(--vc-muted);font-size:.9rem;cursor:pointer;transition:border-color .12s,background .12s;background:var(--vc-panel)}
.vc-dropzone:hover{border-color:var(--vc-accent);color:var(--vc-accent)}
.vc-dropzone.vc-dropzone-over{border-color:var(--vc-accent);background:var(--vc-accent-soft);color:var(--vc-accent)}
.vc-preview-video{width:100%;height:auto;display:block;border-radius:10px;margin-top:10px;background:#000;max-height:280px}

.vc-meta{font-size:.85rem;color:var(--vc-fg);margin:10px 0 0;font-weight:600}
.vc-error{color:var(--vc-err);font-size:.85rem;margin:8px 0 0}
.vc-limit{margin:10px 0 0;padding:10px 12px;background:var(--vc-accent-soft);border:1px dashed var(--vc-accent);border-radius:10px;font-size:.85rem}
.vc-limit-text{color:var(--vc-fg)}
.vc-working{color:var(--vc-accent);font-size:.85rem;margin:8px 0 0;font-weight:600}

.vc-actions{display:flex;gap:8px;flex-wrap:wrap;margin-top:14px;align-items:center}
.vc-btn{padding:9px 16px;border-radius:9px;border:1px solid var(--vc-border);background:var(--vc-surface);color:var(--vc-fg);font:inherit;font-weight:600;cursor:pointer;transition:border-color .12s,color .12s,transform .12s}
.vc-btn:hover{border-color:var(--vc-accent);color:var(--vc-accent);transform:translateY(-1px)}
.vc-btn[disabled]{opacity:.55;cursor:default;transform:none}
.vc-primary{background:linear-gradient(135deg,var(--vc-accent),var(--vc-accent-2));border-color:transparent;color:var(--vc-accent-fg);box-shadow:var(--vc-shadow)}
.vc-primary:hover{color:var(--vc-accent-fg);filter:brightness(1.07)}

.vc-progress-track{width:100%;height:8px;border-radius:999px;background:var(--vc-panel);overflow:hidden;margin-top:10px}
.vc-progress-fill{height:100%;width:0;border-radius:999px;background:linear-gradient(90deg,var(--vc-accent),var(--vc-accent-2));transition:width .2s}

.vc-summary{margin:10px 0 0;padding:14px 16px;background:var(--vc-accent-soft);border-radius:10px}
.vc-headline{font-weight:700;font-size:1.02rem;margin:0 0 4px;color:var(--vc-fg)}
.vc-detail{color:var(--vc-muted);font-size:.83rem;margin:0}

.vc-preset-row{align-items:center;gap:8px}
.vc-preset-label{flex:1;font-size:.85rem;font-weight:600}
.vc-preset-row .vc-btn{padding:5px 12px;font-size:.78rem;border-radius:999px}

.vc-queue-row{align-items:center;gap:8px;border-bottom:1px solid var(--vc-border);padding-bottom:8px;margin-top:8px}
.vc-queue-name{flex:1;font-size:.85rem;font-weight:600}
.vc-queue-status{font-size:.78rem;color:var(--vc-muted);font-weight:700;text-transform:uppercase;letter-spacing:.04em}
.vc-queue-done{color:var(--vc-accent)}
.vc-queue-failed{color:var(--vc-err);text-transform:none;letter-spacing:0}
.vc-queue-row .vc-btn{padding:5px 12px;font-size:.78rem;border-radius:999px}

.vc-upsell{border-radius:14px;padding:16px 18px;background:linear-gradient(135deg,var(--vc-accent-soft),transparent);border:1px dashed var(--vc-accent);color:var(--vc-fg);font-size:.88rem}
.vc-upsell h3{margin:0 0 6px}
.vc-upsell a{color:var(--vc-accent);font-weight:700;text-decoration:none}
.vc-upsell a:hover{text-decoration:underline}

.vc-footer{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:10px;margin-top:26px;padding-top:16px;border-top:1px solid var(--vc-border);color:var(--vc-muted);font-size:.82rem}
.vc-footer-brand{font-weight:700;color:var(--vc-fg)}
.vc-footer-links{display:flex;flex-wrap:wrap;gap:4px 16px}
.vc-footer-link{padding:0;border:none;background:none;color:var(--vc-accent);font:inherit;font-weight:600;cursor:pointer}
.vc-footer-link:hover{text-decoration:underline}
"#;
