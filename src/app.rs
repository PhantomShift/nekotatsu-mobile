#![allow(non_snake_case)]

use apply::Apply;
use dioxus::prelude::*;
use dioxus::logger::tracing::info;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"])]
    async fn invoke(cmd: &str, args: JsValue) -> JsValue;

    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = "invoke")]
    async fn try_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen")]
    async fn event_listen(event: &str, handler: &Closure<dyn FnMut(JsValue) -> ()>) -> JsValue;
}

// For convenience
#[wasm_bindgen]
extern "C" {
    type Store;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "store"], js_name = "load")]
    async fn store_load(path: &str) -> Store;

    #[wasm_bindgen(method)]
    async fn get(this: &Store, key: &str) -> JsValue;

    #[wasm_bindgen(method)]
    async fn set(this: &Store, key: &str, val: JsValue);
}

#[derive(Serialize, Deserialize)]
struct TauriEvent<T> {
    event: String,
    id: f64,
    payload: T,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppSettings {
    pub custom_extensions_url: Option<String>,
}

#[component]
pub fn AppPage(
    page_id: String,
    current_page: Signal<String>,
    children: Element
) -> Element {
    rsx! {
        div {
            hidden: current_page.read().to_owned() != page_id,
            { children }
        }
    }
}

#[component]
pub fn PageSelect(
    mut current_page: Signal<String>,
    ids: Vec<(&'static str, &'static str)>,
) -> Element {
    rsx! {
        div {
            class: "light-contrast",
            "popover": "auto",
            id: "page-select",
            div {
                display: "flex",
                flex_direction: "column",
                for (id, display) in ids {
                    button {
                        "popovertarget": "page-select",
                        onclick: move |_| current_page.set(id.to_string()),
                        {display.to_string()}
                    }
                }
            }
        }
    }
}

#[component]
pub fn LogsPage(
    current_page: Signal<String>,
    mut log: Signal<String>
) -> Element {
    rsx! {
        AppPage {
            current_page: current_page,
            page_id: "logs",
            h1 { "Logs" }
            p {
                class: "light-contrast",
                overflow_y: "scroll",
                height: "200px",
                text_align: "left",
                overflow_wrap: "anywhere",
                padding: "20px",
                pre {
                    "{log}"
                }
            }
            button {
                onclick: move |_| {
                    info!("Clearing log: {}", log.read());
                    log.set(String::new());
                },
                "Clear Logs"
            }
        }
    }
}

#[component]
pub fn SettingsPage(
    settings: Signal<AppSettings>,
    current_page: Signal<String>,
) -> Element {

    let initial_settings = use_resource(|| async move {
        let store = store_load("storage.json").await;
        store.get("settings").await
            .apply(serde_wasm_bindgen::from_value::<AppSettings>)
            .unwrap_or_default()
    });

    rsx! {
        AppPage {
            current_page,
            page_id: "settings",
            h1 { "Settings" }
            form {
                text_align: "left",
                margin: "20px",
                onsubmit: move |ev| {
                    ev.stop_propagation();
                    info!("Got: {:?}", ev.values());
                    // Yucky, will need to change
                    // if more settings are added
                    for (name, val) in ev.values().iter() {
                        if name == "custom_extensions_url" {
                            settings.write().custom_extensions_url = match val.0[0].clone() {
                                val if val.is_empty() => {
                                    None
                                },
                                val => Some(val)
                            };
                        }
                    }
                    spawn(async move {
                        let store = store_load("storage.json").await;
                        let to_save = serde_wasm_bindgen::to_value::<AppSettings>(&settings.read())
                            .expect("failed to save settings");
                        store.set("settings", to_save).await;
                    });
                },
                p {
                    "Custom Tachiyomi Extensions Download URL"
                }
                input {
                    style: "width: 90%;",
                    display: "block",
                    name: "custom_extensions_url",
                    placeholder: "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json",
                    "type": "url",
                    value: match &*initial_settings.read() {
                        Some(settings) => {
                            settings.custom_extensions_url.clone().unwrap_or_default()
                        },
                        None => { String::new() }
                    }
                }
                button { "Save" }
            }
        }
    }
}

pub fn App() -> Element {
    let mut picked_backup = use_signal(|| String::new());
    let mut picked_save_path = use_signal(|| String::new());
    let mut logs = use_signal(|| String::new());
    let mut settings = use_signal(|| AppSettings::default());
    let current_page = use_signal(|| String::from("convert"));
    
    let log_coroutine = use_coroutine(move |mut rx: UnboundedReceiver<String>| async move {
        while let Some(msg) = rx.next().await {
            info!("{}", &msg);
            logs.write().extend([&msg, "\n"]);
        }
    });

    let on_logged = move |event: JsValue| {
        let event = serde_wasm_bindgen::from_value::<TauriEvent<String>>(event)
            .expect("event should have sent a string");
        log_coroutine.send(event.payload);
    };

    use_future(move || async move {
        let log_closure = Closure::<dyn FnMut(JsValue) -> ()>::new(on_logged);
        event_listen("nekotatsu_log", &log_closure).await;
        log_closure.forget();
    });

    use_future(move || async move {
        let store = store_load("storage.json").await;
        let loaded_settings = store.get("settings").await
            .apply(serde_wasm_bindgen::from_value::<AppSettings>)
            .unwrap_or_default();

        *settings.write() = loaded_settings;
    });

    // This seems *really* weird/overkill but my brain is too small/lazy
    // to do this properly with an arc mutex or whatever
    // and shouldn't realistically matter
    let mut busy =  use_signal(|| false);

    macro_rules! busy_run {
        ($task:block, $busy_message:expr) => {
            if !*busy.read() {
                busy.set(true);
                spawn(async move {
                    {
                        $task
                    };
                    busy.set(false);
                });
            } else {
                spawn(async move {
                    invoke("plugin:dialog|message",
                        serde_wasm_bindgen::to_value(
                            &json!({
                                "message": $busy_message,
                                "options": {
                                    "title": "Busy"
                                }
                            })
                        ).expect("should be valid json")
                    ).await;
                });
            }
        };
    }

    rsx! {
        link { rel: "stylesheet", href: "/assets/styles.css" }
        main {
            class: "container",
            AppPage {
                current_page: current_page,
                page_id: "convert",

                h1 { "Nekotatsu" }
                div {
                    button {
                        onclick: move |_| busy_run!({
                            let _ = try_invoke("download_tachi_sources", JsValue::null()).await;
                        }, "Cannot download, busy with other operations"),
                        "Download Tachiyomi Sources"
                    },
                    button {
                        onclick: move |_| busy_run!({
                            let _ = try_invoke("update_kotatsu_parsers", JsValue::null()).await;
                        }, "Cannot update, busy with other operations"),
                        "Update Kotatsu Parsers"
                    }
                }
                div {
                    display: "flex",
                    flex_direction: "column",
                    button {
                        onclick: move |_| busy_run!({
                            let res = invoke("pick_backup", JsValue::null()).await;
                            if let Some(path) = res.as_string() {
                                picked_backup.set(path);
                            }
                        }, "Busy with other operations"),
                        "Pick Backup"
                    }
                    input {
                        readonly: true,
                        width: "100%",
                        overflow_wrap: "anywhere",
                        value: "{picked_backup}"
                    }
                    button {
                        onclick: move |_| busy_run!({
                            let res = invoke("pick_save_path", JsValue::null()).await;
                            if let Some(path) = res.as_string() {
                                picked_save_path.set(path);
                            }
                        }, "Busy with other operations"),
                        "Pick Save Path"
                    }
                    input {
                        readonly: true,
                        width: "100%",
                        overflow_wrap: "anywhere",
                        value: "{picked_save_path}"
                    }
                }
                div {
                    button {
                        onclick: move |_| busy_run!({
                            let _ = try_invoke("convert_backup", JsValue::null()).await;
                        }, "Busy with other operations, please wait"),
                        "Convert"
                    }
                }
            }
            LogsPage {
                log: logs,
                current_page
            }
            SettingsPage {
                current_page,
                settings
            }
            AppPage {
                current_page,
                page_id: "about",
                div {
                    h1 { "About" }
                    h2 { "Nekotatsu Mobile" }
                    p {
                        "Version: " {env!("CARGO_PKG_VERSION")}
                    }
                    img {
                        width: "200px",
                        src: "/assets/logo.svg"
                    }
                    p {
                        "A GUI frontend for nekotatsu, a tool to convert Tachiyomi backups"
                        " into backups readable by Kotatsu."
                    }
                }
            }
            PageSelect {
                current_page,
                ids: vec![
                    ("convert", "Convert"),
                    ("logs", "Logs"),
                    ("settings", "Settings"),
                    ("about", "About"),
                ],
            }
            button {
                position: "fixed",
                left: 0,
                top: 0,
                "popovertarget": "page-select",
                "⚙️"
            }
        }
    }
}