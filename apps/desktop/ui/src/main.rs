use leptos::{mount::mount_to_body, prelude::*};
use serde::Deserialize;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::{spawn_local, JsFuture};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch, js_name = invoke)]
    async fn invoke_tauri(command: &str) -> Result<JsValue, JsValue>;
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct BridgeStatus {
    schema_version: String,
    url: String,
    token: String,
    status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LoadState {
    Loading,
    Ready(BridgeStatus),
    Failed(String),
}

fn main() {
    mount_to_body(|| view! { <App /> });
}

#[component]
fn App() -> impl IntoView {
    let (state, set_state) = signal(LoadState::Loading);

    Effect::new(move |_| {
        spawn_local(async move {
            match load_bridge_status().await {
                Ok(status) => set_state.set(LoadState::Ready(status)),
                Err(error) => set_state.set(LoadState::Failed(error)),
            }
        });
    });

    let status_label = move || match state.get() {
        LoadState::Loading => "Starting".to_owned(),
        LoadState::Ready(status) => status.status,
        LoadState::Failed(_) => "Unavailable".to_owned(),
    };
    let status_class = move || match state.get() {
        LoadState::Failed(_) => "status error",
        LoadState::Loading | LoadState::Ready(_) => "status",
    };
    let bridge_url = move || match state.get() {
        LoadState::Ready(status) => status.url,
        LoadState::Loading | LoadState::Failed(_) => String::new(),
    };
    let bridge_token = move || match state.get() {
        LoadState::Ready(status) => status.token,
        LoadState::Loading | LoadState::Failed(_) => String::new(),
    };
    let schema_version = move || match state.get() {
        LoadState::Ready(status) => status.schema_version,
        LoadState::Loading | LoadState::Failed(_) => String::new(),
    };
    let error_text = move || match state.get() {
        LoadState::Failed(error) => error,
        LoadState::Loading | LoadState::Ready(_) => String::new(),
    };
    let is_ready = move || matches!(state.get(), LoadState::Ready(_));

    view! {
        <main>
            <div class="shell">
                <div class="topbar">
                    <h1>"Screen Sidekick"</h1>
                    <span class=status_class>{status_label}</span>
                </div>
                <section class="panel">
                    <div class="field">
                        <label for="bridge-url">"Bridge URL"</label>
                        <div class="copy-row">
                            <input id="bridge-url" readonly prop:value=bridge_url />
                            <button
                                type="button"
                                disabled=move || !is_ready()
                                on:click=move |_| copy_to_clipboard(bridge_url())
                            >
                                "Copy"
                            </button>
                        </div>
                    </div>
                    <div class="field">
                        <label for="bridge-token">"Bearer Token"</label>
                        <div class="copy-row">
                            <input id="bridge-token" readonly prop:value=bridge_token />
                            <button
                                type="button"
                                disabled=move || !is_ready()
                                on:click=move |_| copy_to_clipboard(bridge_token())
                            >
                                "Copy"
                            </button>
                        </div>
                    </div>
                    <div class="field">
                        <label for="schema-version">"Status Schema"</label>
                        <input id="schema-version" readonly prop:value=schema_version />
                    </div>
                    <Show when=move || matches!(state.get(), LoadState::Failed(_))>
                        <div class="status error">{error_text}</div>
                    </Show>
                </section>
            </div>
        </main>
    }
}

async fn load_bridge_status() -> Result<BridgeStatus, String> {
    let value = invoke_tauri("get_bridge_status")
        .await
        .map_err(|error| js_error_to_string(&error))?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

fn copy_to_clipboard(text: String) {
    if text.is_empty() {
        return;
    }

    let Some(window) = web_sys::window() else {
        return;
    };

    let promise = window.navigator().clipboard().write_text(&text);
    spawn_local(async move {
        let _ = JsFuture::from(promise).await;
    });
}

fn js_error_to_string(value: &JsValue) -> String {
    value
        .as_string()
        .unwrap_or_else(|| "Tauri command failed".to_owned())
}
