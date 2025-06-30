use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{http::StatusCode, AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_fs::{FilePath, FsExt, OpenOptions};
use tauri_plugin_store::StoreExt;

static TACHI_DOWNLOAD_LINK: &str =
    "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.min.json";
static KOTATSU_DOWNLOAD_LINK: &str =
    "https://github.com/KotatsuApp/kotatsu-parsers/archive/refs/heads/master.zip";

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct AppSettings {
    pub custom_extensions_url: Option<String>,
}

#[derive(Default)]
struct PathState {
    backup_path: Option<FilePath>,
    save_path: Option<FilePath>,
}

struct AppLogger {
    app: AppHandle,
}

struct AppLoggerMaker(AppHandle);

impl AppLogger {
    fn log_info(&self, message: &str) {
        self.app
            .emit("nekotatsu_log", message)
            .expect("emit should work")
    }
}

impl std::io::Write for AppLogger {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = String::from_utf8(buf.to_vec()).map_err(std::io::Error::other)?;
        self.app
            .emit("nekotatsu_log", msg.trim())
            .map_err(std::io::Error::other)
            .and(Ok(buf.len()))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl tracing_subscriber::fmt::MakeWriter<'_> for AppLoggerMaker {
    type Writer = std::io::LineWriter<AppLogger>;
    fn make_writer(&self) -> Self::Writer {
        std::io::LineWriter::new(AppLogger {
            app: self.0.clone(),
        })
    }
}

// this is kinda yucky but whatever
async fn download_file(app: &AppHandle, link: &str, destination: &Path) -> Result<File, String> {
    let response = tauri_plugin_http::reqwest::get(link).await;
    let result = match response {
        Ok(mut resp) => {
            if resp.status() == StatusCode::OK {
                let options = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .to_owned();
                let mut handle = app
                    .fs()
                    .open(destination, options)
                    .expect("failed to open file path for saving; do we have write permissions?");
                let mut writer = BufWriter::new(&mut handle);
                while let Some(bytes) = resp.chunk().await.map_err(|e| e.to_string())? {
                    writer.write_all(&bytes).map_err(|e| e.to_string())?;
                }
                drop(writer);

                app.dialog().message("Download complete!").blocking_show();

                Ok(handle)
            } else {
                Err("non-OK status code".into())
            }
        }
        Err(e) => return Err(e.to_string()),
    };
    result.inspect_err(|e| {
        app.dialog()
            .message(format!("Error downloading file: {e}"))
            .blocking_show();
    })
}

#[tauri::command]
async fn download_tachi_sources(app: AppHandle) -> Result<(), String> {
    let mut path = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    path.extend(&["tachi_sources.json"]);

    if path.exists() {
        let overwrite = app
            .dialog()
            .message("Sources file already exist; overwrite?")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show();
        if !overwrite {
            return Ok(());
        }
    }

    let store = app.store("storage.json").expect("store should be openable");

    let link = store
        .get("settings")
        .map(|val| serde_json::from_value::<AppSettings>(val).unwrap_or_default())
        .and_then(|settings| settings.custom_extensions_url)
        .unwrap_or(TACHI_DOWNLOAD_LINK.to_string());

    download_file(&app, &link, &path).await.map(|_| ())
}

#[tauri::command]
async fn update_kotatsu_parsers(app: AppHandle) -> Result<(), String> {
    let mut path = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    path.extend(&["kotatsu_parsers.zip"]);

    let should_download = !path.exists() || {
        app.dialog()
            .message("Parsers repo already downloaded; download again?")
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    };
    if should_download {
        let mut zipfile = download_file(&app, KOTATSU_DOWNLOAD_LINK, &path).await?;
        zipfile.flush().map_err(|e| e.to_string())?;
        drop(zipfile);
    }

    let zipfile = app
        .fs()
        .open(&path, OpenOptions::new().read(true).to_owned())
        .map_err(|e| e.to_string())?;

    let mut json_path = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    json_path.extend(&["kotatsu_parsers.json"]);

    let parsers_file = app
        .fs()
        .open(
            json_path,
            OpenOptions::new()
                .write(true)
                .truncate(true)
                .create(true)
                .to_owned(),
        )
        .map_err(|e| e.to_string())?;
    nekotatsu_core::kotatsu::update_parsers(&zipfile, &parsers_file).map_err(|e| {
        app.dialog()
            .message(format!("Failed to update parsers: {e}"))
            .blocking_show();
        e.to_string()
    })?;

    Ok(())
}

#[tauri::command]
async fn pick_backup(
    app: AppHandle,
    state: tauri::State<'_, Mutex<PathState>>,
) -> Result<Option<String>, String> {
    if let Some(file_path) = app.dialog().file().blocking_pick_file() {
        state
            .lock()
            .as_mut()
            .map_err(|e| e.to_string())?
            .backup_path
            .replace(file_path.clone());
        Ok(Some(file_path.to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn pick_save_path(
    app: AppHandle,
    state: tauri::State<'_, Mutex<PathState>>,
) -> Result<Option<String>, String> {
    if let Some(file_path) = app
        .dialog()
        .file()
        .add_filter("Zip File", &["zip"])
        .blocking_save_file()
    {
        let extension_matches = match file_path.clone() {
            FilePath::Path(path) => path.extension().is_some_and(|ext| ext == "zip"),
            FilePath::Url(url) => url.as_str().ends_with(".zip"),
        };
        if !extension_matches {
            app.dialog()
                .message("File must be a .zip file")
                .blocking_show();
            return Ok(None);
        }
        state
            .lock()
            .as_mut()
            .map_err(|e| e.to_string())?
            .save_path
            .replace(file_path.clone());
        Ok(Some(file_path.to_string()))
    } else {
        Ok(None)
    }
}

#[tauri::command]
async fn convert_backup(
    app: AppHandle,
    state: tauri::State<'_, Mutex<PathState>>,
) -> Result<(), String> {
    let mut sources_path = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    sources_path.extend(&["tachi_sources.json"]);
    if !sources_path.exists() {
        app.dialog()
            .message("Tachiyomi source list not downloaded")
            .blocking_show();
        return Ok(());
    }

    let mut parsers_path = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    parsers_path.extend(&["kotatsu_parsers.json"]);
    if !parsers_path.exists() {
        app.dialog()
            .message("Kotatsu parsers list not downloaded")
            .blocking_show();
        return Ok(());
    }

    let state = state.lock().map_err(|e| e.to_string())?;
    match (state.backup_path.as_ref(), state.save_path.as_ref()) {
        (Some(backup_path), Some(save_path)) => {
            let backup_file = app
                .fs()
                .open(
                    backup_path.clone(),
                    OpenOptions::new().read(true).to_owned(),
                )
                .expect("backup file should exist");
            let backup = nekotatsu_core::decode_neko_backup(backup_file).map_err(|e| {
                app.dialog().message(format!("Error decoding backup, was this a valid tachiyomi backup? Original error: {e:?}"))
                    .blocking_show();
                e.to_string()
            })?;

            let sources_file = app
                .fs()
                .open(sources_path, OpenOptions::new().read(true).to_owned())
                .expect("sources file should exist");
            let parsers_file = app
                .fs()
                .open(parsers_path, OpenOptions::new().read(true).to_owned())
                .expect("parsers file should exist");

            let converter =
                nekotatsu_core::MangaConverter::try_from_files(parsers_file, sources_file)
                    .map_err(|e| {
                        app.dialog()
                            .message(format!("Error source/parsers files: {e:?}"))
                            .blocking_show();
                        e.to_string()
                    })?;

            let logger = AppLogger { app: app.clone() };
            let result = nekotatsu_core::tracing::subscriber::with_default(
                tracing_subscriber::fmt::fmt()
                    .compact()
                    .with_writer(AppLoggerMaker(app.clone()))
                    .with_ansi(false)
                    .with_file(false)
                    .without_time()
                    .finish(),
                || converter.convert_backup(backup, "Library", &mut |_| true),
            );

            let save_file = app
                .fs()
                .open(
                    save_path.clone(),
                    OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .create(true)
                        .to_owned(),
                )
                .map_err(|e| {
                    app.dialog()
                        .message(format!("Error saving converted backup: {e:?}"))
                        .blocking_show();
                    e.to_string()
                })?;

            let options = zip::write::FileOptions::<()>::default();
            let mut writer = zip::ZipWriter::new(save_file);
            for (name, entry) in [
                ("history", serde_json::to_string_pretty(&result.history)),
                (
                    "categories",
                    serde_json::to_string_pretty(&result.categories),
                ),
                (
                    "favourites",
                    serde_json::to_string_pretty(&result.favourites),
                ),
                ("bookmarks", serde_json::to_string_pretty(&result.bookmarks)),
                (
                    "index",
                    serde_json::to_string_pretty(&[
                        nekotatsu_core::kotatsu::KotatsuIndexEntry::generate(),
                    ]),
                ),
            ] {
                match entry {
                    Ok(json) if json.trim() != "[]" => {
                        writer
                            .start_file(name, options)
                            .map_err(|e| e.to_string())?;
                        writer
                            .write_all(json.as_bytes())
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(_) => {
                        logger
                            .log_info(&format!("{name} is empty, ommitted from converted backup"));
                    }
                    Err(e) => {
                        logger.log_info(&format!(
                            "[WARNING] Error occurred processing {name}, ommitted from converted backup, original error: {e}"
                        ));
                    }
                }
            }

            writer.finish().map_err(|e| e.to_string())?;
            app.dialog()
                .message("Conversion completed!")
                .blocking_show();
        }
        (_, None) => {
            app.dialog().message("Save path not set").blocking_show();
        }
        (None, _) => {
            app.dialog().message("Backup not chosen").blocking_show();
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Mutex::new(PathState::default()))
        .invoke_handler(tauri::generate_handler![
            download_tachi_sources,
            update_kotatsu_parsers,
            pick_backup,
            pick_save_path,
            convert_backup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
