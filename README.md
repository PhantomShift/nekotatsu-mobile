# Nekotatsu Mobile

GUI frontend for [nekotatsu](https://github.com/PhantomShift/nekotatsu),
a tool for converting Tachiyomi backups into backups readable by [Kotatsu](https://github.com/KotatsuApp/Kotatsu).

I am aware that the UI is *incredibly* scuffed, it is very much work in progress.

Also note that this can double as a frontend for desktop, but is not the main
focus of the project at this stage. Regardless, it is trivial to try out with
`tauri dev` (or whatever the appropriate command is for your installation of
the [tauri-cli](https://tauri.app/reference/cli/)), as I doubt most users will
need to use this tool a lot over an extended period of time.

## Usage

To convert, download the sources and parsers lists,
then pick the backup and save paths and hit convert.

If an extension list other than the Keiyoshi one was used when you created your Tachiyomi backup,
open the settings and set the url for the relevant `index.min.json` to download.

> This will be updated to accept your own local files instead in the future
