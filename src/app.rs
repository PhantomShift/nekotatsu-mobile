#![allow(non_snake_case)]

use std::sync::LazyLock;

use apply::Apply;
use bevy_reflect::{GetField, Reflect, StructInfo, Typed};
use dioxus::logger::tracing::info;
use dioxus::prelude::*;
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

#[derive(Reflect)]
struct EntryPlaceholder(&'static str);
#[derive(Reflect)]
struct EntryTitle(&'static str);
#[derive(Reflect)]
struct EntryFileName(&'static str);

#[derive(Debug, Reflect, Serialize, Deserialize, Clone, Default)]
pub struct AppSettings {
    #[reflect(@EntryPlaceholder("https://github.com/keiyoushi/extensions/raw/refs/heads/repo/index.min.json"))]
    #[reflect(@EntryTitle("Tachiyomi Sources URL"))]
    #[reflect(@EntryFileName("tachi_sources.json"))]
    pub custom_extensions_url: Option<String>,

    #[reflect(@EntryPlaceholder("https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip"))]
    #[reflect(@EntryTitle("Kotatsu Parsers URL"))]
    #[reflect(@EntryFileName("kotatsu_parsers.zip"))]
    pub custom_parsers_url: Option<String>,

    #[reflect(@EntryPlaceholder("https://raw.githubusercontent.com/phantomshift/nekotatsu/master/nekotatsu-core/src/correction.luau"))]
    #[reflect(@EntryTitle("Fixer Script URL"))]
    #[reflect(@EntryFileName("correction.luau"))]
    pub custom_fixer_url: Option<String>,
}

static APP_SETTINGS_INFO: LazyLock<&StructInfo> = LazyLock::new(|| {
    AppSettings::type_info()
        .as_struct()
        .expect("AppSettings should be a struct")
});

macro_rules! json_value {
    ($($value:tt)+) => {
        serde_wasm_bindgen::to_value(&json!($($value)+)).expect("should be valid json")
    };
}

macro_rules! busy_run {
    ($task:block, $busy_signal:ident, $busy_message:expr) => {
        if !*$busy_signal.read() {
            $busy_signal.set(true);
            spawn(async move {
                {
                    $task
                };
                $busy_signal.set(false);
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

#[component]
pub fn AppPage(page_id: String, current_page: Signal<String>, children: Element) -> Element {
    rsx! {
        div {
            hidden: current_page.read().to_owned() != page_id,
            height: "100%",
            padding: "1em",
            {children}
        }
    }
}

#[component]
pub fn PageSelect(
    mut current_page: Signal<String>,
    ids: Vec<(&'static str, &'static str)>,
) -> Element {
    rsx! {
        div { class: "light-contrast", "popover": "auto", id: "page-select",
            div { display: "flex", flex_direction: "column",
                for (id , display) in ids {
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
pub fn LogsPage(current_page: Signal<String>, mut log: Signal<String>) -> Element {
    rsx! {
        AppPage { current_page, page_id: "logs",
            div {
                height: "100%",
                align_content: "center",
                display: "flex",
                flex_direction: "column",
                h1 { "Logs" }
                p {
                    display: "flex",
                    flex_grow: 1,
                    class: "light-contrast",
                    overflow: "auto",
                    // height: "200px",
                    text_align: "left",
                    overflow_wrap: "anywhere",
                    padding: "20px",
                    pre { "{log}" }
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
}

#[component]
pub fn SettingsPage(settings: Signal<AppSettings>, current_page: Signal<String>) -> Element {
    let initial_settings = use_resource(move || async move {
        let store = store_load("storage.json").await;
        store
            .get("settings")
            .await
            .apply(serde_wasm_bindgen::from_value::<AppSettings>)
            .unwrap_or_default()
    });

    #[component]
    fn SettingsEntry(name: String, initial_settings: Resource<AppSettings>) -> Element {
        rsx! {
            p {
                {
                    APP_SETTINGS_INFO
                        .field(&name)
                        .and_then(|field| field.get_attribute::<EntryTitle>())
                        .expect("title")
                        .0
                }
            }
            input {
                style: "width: 90%;",
                display: "block",
                name: name.as_str(),
                placeholder: APP_SETTINGS_INFO
                    .field(&name)
                    .and_then(|field| field.get_attribute::<EntryPlaceholder>())
                    .map(|placeholder| placeholder.0)
                    .unwrap_or_default(),
                "type": "url",
                value: initial_settings
                    .read()
                    .as_ref()
                    .and_then(|settings| settings.get_field::<Option<String>>(&name))
                    .and_then(|field| field.clone()),
            }
        }
    }

    let entries = APP_SETTINGS_INFO.iter().map(|field| {
        rsx! {
            SettingsEntry { name: field.name(), initial_settings }
        }
    });

    rsx! {
        AppPage { current_page, page_id: "settings",
            h1 { "Settings" }
            form {
                text_align: "left",
                margin: "20px",
                onsubmit: move |ev| {
                    ev.stop_propagation();
                    let mut current_settings = settings.write();
                    for (name, mut val) in ev.values().into_iter() {
                        if let Some(field) = current_settings.get_field_mut::<Option<String>>(&name)
                        {
                            *field = val.0.drain(0..).next();
                        }
                    }
                    drop(current_settings);
                    spawn(async move {
                        let store = store_load("storage.json").await;
                        let to_save = serde_wasm_bindgen::to_value::<AppSettings>(&settings.read())
                            .expect("failed to save settings");
                        store.set("settings", to_save).await;
                    });
                },
                {entries}
                button { "Save" }
            }
        }
    }
}

#[component]
fn DownloadPage(
    settings: Signal<AppSettings>,
    current_page: Signal<String>,
    busy: Signal<bool>,
) -> Element {
    let entries: Vec<_> = APP_SETTINGS_INFO
        .iter()
        .map(|field| {
            let mut status = use_signal(|| false);
            let file_name = field.get_attribute::<EntryFileName>().expect("setting missing file name").0;
            use_future(move || async move {
                let exists = try_invoke(
                    "file_exists",
                    json_value!({ "fileName": file_name }),
                )
                .await.unwrap().as_bool();
                *status.write() = exists.is_some_and(|e| e);
            });
            rsx! {
                div {
                    display: "flex",
                    align_content: "center",
                    align_items: "center",
                    justify_content: "stretch",
                    span { {if *status.read() { "✅" } else { "❌" }} }
                    p { flex_grow: "1", align_content: "left",
                        {field.get_attribute::<EntryTitle>().expect("setting mission title").0}
                    }
                    button {
                        // Holy minified JavaScript Batman, this is what Dioxus auto format writes!
                        onclick: move |ev| {
                            ev.stop_propagation();
                            busy_run!(
                                { let link = settings.read().get_field::< Option < String >> (field.name())
                                .map(Option::to_owned).or_else(|| field.get_attribute::< EntryPlaceholder >
                                ().map(| placeholder | Some(placeholder.0.to_string()))).flatten()
                                .expect("failed to get link"); let _ = try_invoke("request_download",
                                json_value!({ "fileName" : file_name, "link" : link })). await; let exists =
                                try_invoke("file_exists", json_value!({ "fileName" : file_name })). await
                                .unwrap().as_bool(); * status.write() = exists.is_some_and(| e | e); }, busy,
                                "Cannot download, currently busy."
                            )
                        },
                        "Download"
                    }
                }
            }
        })
        .collect();

    rsx! {
        AppPage { current_page, page_id: "download", {entries.iter()} }
    }
}

pub fn App() -> Element {
    let mut picked_backup = use_signal(String::new);
    let mut picked_save_path = use_signal(String::new);
    let mut logs = use_signal(String::new);
    let mut settings = use_signal(AppSettings::default);
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
        let log_closure = Closure::<dyn FnMut(JsValue)>::new(on_logged);
        event_listen("nekotatsu_log", &log_closure).await;
        log_closure.forget();
    });

    use_future(move || async move {
        let store = store_load("storage.json").await;
        let loaded_settings = store
            .get("settings")
            .await
            .apply(serde_wasm_bindgen::from_value::<AppSettings>)
            .unwrap_or_default();

        *settings.write() = loaded_settings;
    });

    // This seems *really* weird/overkill but my brain is too small/lazy
    // to do this properly with an arc mutex or whatever
    // and shouldn't realistically matter
    let mut busy = use_signal(|| false);

    rsx! {
        link { rel: "stylesheet", href: "/assets/styles.css" }
        main { class: "container", height: "100%",
            AppPage { current_page, page_id: "convert",

                h1 { "Nekotatsu" }
                div { display: "flex", flex_direction: "column",
                    button {
                        onclick: move |_| {
                            busy_run!(
                                { let res = invoke("pick_backup", JsValue::null()). await; if let Some(path)
                                = res.as_string() { picked_backup.set(path); } }, busy,
                                "Busy with other operations"
                            )
                        },
                        "Pick Backup"
                    }
                    input {
                        readonly: true,
                        width: "100%",
                        overflow_wrap: "anywhere",
                        value: "{picked_backup}",
                    }
                    button {
                        onclick: move |_| {
                            busy_run!(
                                { let res = invoke("pick_save_path", JsValue::null()). await; if let
                                Some(path) = res.as_string() { picked_save_path.set(path); } }, busy,
                                "Busy with other operations"
                            )
                        },
                        "Pick Save Path"
                    }
                    input {
                        readonly: true,
                        width: "100%",
                        overflow_wrap: "anywhere",
                        value: "{picked_save_path}",
                    }
                }
                div {
                    button {
                        onclick: move |_| {
                            busy_run!(
                                { let _ = try_invoke("convert_backup", JsValue::null()). await; }, busy,
                                "Busy with other operations, please wait"
                            )
                        },
                        "Convert"
                    }
                }
            }
            DownloadPage { settings, current_page, busy }
            LogsPage { log: logs, current_page }
            SettingsPage { current_page, settings }
            AppPage { current_page, page_id: "about",
                div {
                    h1 { "About" }
                    h2 { "Nekotatsu Mobile" }
                    p {
                        "Version: "
                        {env!("CARGO_PKG_VERSION")}
                    }
                    img { width: "200px", src: "/assets/logo.svg" }
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
                    ("download", "Download"),
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
