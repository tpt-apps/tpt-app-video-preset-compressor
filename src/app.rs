//! Free/Pro DOM UI (tpt-appfront-dom).
//!
//! Phase 0 placeholder: proves the hub contract (mount + start + marker
//! div) end to end. The real UI replaces `mount_app`'s body in Phase 2 —
//! the architecture will follow tpt-app-fea-lite's proven split
//! (mount-once shell + reactive results + raw web-sys escape hatches).

use std::rc::Rc;

use tpt_appfront_core::{Signal, UITree};
use wasm_bindgen::prelude::*;

#[derive(Debug, Clone)]
enum Msg {
    Ping,
}

pub fn mount_app(container: &web_sys::Element) -> Result<(), JsValue> {
    let status: Signal<String> = Signal::new("scaffold ok".to_string());

    let status_for_dispatch = status.clone();
    let dispatch: Rc<dyn Fn(Msg)> = Rc::new(move |msg| match msg {
        Msg::Ping => status_for_dispatch.set("clicked".to_string()),
    });

    let ui: UITree<Msg> = UITree::container(|c| {
        c.heading(2, "TPT Video Compressor");
        c.text("Phase 0 scaffold — hub contract mounted.");
        c.button("Ping").on_click(Msg::Ping);
    });

    let root = tpt_appfront_dom::mount(container, &ui, dispatch)?;
    std::mem::forget(root);
    Ok(())
}
