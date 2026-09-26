<p align="center">
  <img src="https://github.com/nnapkin12/AudiosPlayer/blob/main/public/audios.png" alt="Audios!" width="360">
</p>

# Contributing

Audios! splits UI from native work. React renders state and calls `invoke`. Disk, audio, child processes, tags, and the LAN remote live in Rust.

Read [`docs/architecture.md`](docs/architecture.md) before changing search, the decoder, or the remote. Several YouTube and rodio workarounds look optional. They are not.

## Where a change goes

- Files, playback, yt-dlp, or ffmpeg: add or extend a command in [`src-tauri/src/commands/mod.rs`](src-tauri/src/commands/mod.rs). Do not do that work in the webview.
- [`src/lib/api.ts`](src/lib/api.ts) is the only module that talks to Tauri. Call it from the feature views.
- The remote web page is not a React route. It is [`src-tauri/src/remote_page.html`](src-tauri/src/remote_page.html), and [`src-tauri/src/remote.rs`](src-tauri/src/remote.rs) calls `Player` directly.
- Discover groups tracks the UI already scanned. It does not need its own command.

## Commands

Player, tag, appearance, search, and `remote_start` / `remote_stop` / `remote_status` are the desktop IPC surface.

- Keep payloads `camelCase` via serde so they match [`src/lib/types.ts`](src/lib/types.ts). Add a field on both sides in the same change.
- `AppError` serializes as a string. In the UI, unwrap it with `errorMessage()` from [`src/lib/format.ts`](src/lib/format.ts). Do not use `error instanceof Error` on an invoke failure.

## Player

- Playback goes through the `PlayerEngine` trait in [`src-tauri/src/player/engine.rs`](src-tauri/src/player/engine.rs) (rodio + Symphonia).
- The parametric EQ in [`src-tauri/src/eq.rs`](src-tauri/src/eq.rs) wraps each decoder on the `audios-rodio` thread. One band list feeds the tone sliders and Advanced. Do not add a second graphic-EQ processor.
- Queue order, repeat, and shuffle stay in [`src-tauri/src/player/queue.rs`](src-tauri/src/player/queue.rs). Keep those tests next to the logic.
- Do not persist a resume offset. Leaving a track starts it at 0:00 next time. Speed is remembered.
- `play_folder_file` queues the other audio files in that folder, capped at 2,000. Above the cap, only the chosen file is queued. The remote uses this when the song is not already in the queue.

## Library and playlists

- A library root and a playlist folder stay linked to the disk. [`src-tauri/src/watch.rs`](src-tauri/src/watch.rs) notices adds and deletes. Do not snapshot a folder into a file list.
- [`src-tauri/src/relink.rs`](src-tauri/src/relink.rs) retargets a moved path when the name is unique. Otherwise the item stays listed so the UI can ask to Locate it.
- Removing one song from a playlist excludes that file and leaves the folder linked.
- Playlist pictures are JPEGs next to `state.json` in `playlist-covers/` (`{id}.jpg` custom, `{id}.auto.jpg` mosaic). The picker sends a filesystem path. Rust reads the image. Do not send picture bytes through IPC.

## Search

[`src-tauri/src/search.rs`](src-tauri/src/search.rs) lists with `--flat-playlist`. Text search is `ytsearch`, then `scsearch`. A pasted URL is passed through.

- Prefer `webpage_url`. Do not build a YouTube link from an id. A SoundCloud id is not a YouTube watch URL.
- Do not feed YouTube AAC `.m4a` straight to rodio. Do not pin `-f` to a named format such as `bestaudio[ext=m4a]`.
- `mediaconnect` is for YouTube URLs only.
- Child processes go through `spawn_tool`, so an AppImage does not leak its Python and GTK env into host yt-dlp, ffmpeg, or curl.

The workaround table in the architecture doc is the list of things not to delete.

## Tags

[`src-tauri/src/tags/mod.rs`](src-tauri/src/tags/mod.rs) writes a sibling temp that keeps the audio extension, then replaces the original. Do not write tags in place. Not every container has the same frames.

MP4 and M4A reads and writes go through [`src-tauri/src/mp4_recover.rs`](src-tauri/src/mp4_recover.rs) when `mdat` uses a 64-bit size. Audio samples are not rewritten.

`search_covers` returns four YouTube thumbnails. `add_cover_from_url` fetches the image with curl. Do not download audio for artwork.

## Web remote

Started from Settings. Idle until `remote_start`. Sound stays on this computer.

- Do not serve the React app, file paths, or yt-dlp from this server.
- Every request needs the pairing code. Search results are ids from the in-memory index. Play looks up the path on the server.
- Now-playing for the phone is `Player::transport`. It skips the queue, the folder tree, and the EQ.
- Stop must drop the song index and the cached cover.

## UI

- Chrome lives in `src/features/shell`. Window buttons stay on the top right.
- The window is frameless and the UI fills the client area. Do not add inset frame padding. Stay on WebKit-safe CSS.
- Player, Search, Tags, and Settings stay mounted and toggle with `hidden`.

## Comments

Use normal engineering terms (extractor client, remux, decoder init). Do not leave chat leftovers or notes aimed at one person.

## Run

```bash
npm install
npm run tauri dev
```

Playback and tags need the Tauri shell. `npm run dev` alone is a preview. Search also needs a current yt-dlp, ffmpeg, and curl on the machine.

## Checks

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

Put Rust tests next to the module they cover. UI tests are Vitest files beside the code (`src/lib/*.test.ts`, `src/features/player/catalog.test.ts`).
