<p align="center">
  <img src="https://github.com/nnapkin12/AudiosPlayer/blob/main/public/audios.png" alt="Audios!" width="360">
</p>

# Architecture

```
React (WebKitGTK)
  player   search   tags   settings
           |
           +-- src/lib/api.ts (only Tauri invoke)
                  |
           Tauri commands
     +------------+-------------+------------+--------------+
     |            |             |            |              |
  Player       Search        Tag IO      Persist         Remote
  rodio        yt-dlp        lofty       state.json      std::net
  Symphonia    ffmpeg        mp4_recover  themes         phone page
  walkdir      curl                       playlist covers

  Library roots, folder watch, and relink sit beside Player.
  Linux only: MPRIS (souvlaki) and the AppImage desktop entry.
```

Disk, audio, child processes, and tags live in Rust. The webview renders state and calls `invoke`. A change that needs files, audio, or a process gets a command in [`src-tauri/src/commands/mod.rs`](../src-tauri/src/commands/mod.rs).

## Startup

[`src-tauri/src/lib.rs`](../src-tauri/src/lib.rs) loads state, then:

- `playlists::sync` and `library::prune_missing` run before the window is useful.
- `Player` starts the rodio thread. On Linux, MPRIS starts with it.
- `Remote` is created idle. Nothing listens until Settings calls `remote_start`.
- `watch::spawn` follows library and playlist folders.
- An AppImage writes its own desktop entry. A `.deb` already has one.
- Command-line files and folders (`xdg-open song.mp3`, Open With) go through `launch.rs`. A second instance is refused by `tauri-plugin-single-instance` and the paths open in the running window.

To remove an AppImage menu entry: delete `~/.local/share/applications/com.audios.desktop.desktop` and `~/.local/share/icons/hicolor/128x128/apps/com.audios.desktop.png`, then run `update-desktop-database ~/.local/share/applications` if that tool is installed.

## Frontend

- [`src/App.tsx`](../src/App.tsx) — frameless chrome, `player://state` / `player://tick`, `library://changed`, transport keys.
- [`src/store/useAppStore.ts`](../src/store/useAppStore.ts) — last snapshot, cover, tab, status line.
- [`src/lib/api.ts`](../src/lib/api.ts) — the only module that talks to Tauri.
- [`src/lib/format.ts`](../src/lib/format.ts) — `errorMessage()` unwraps Tauri failures. Command errors arrive as strings, not `Error` objects. Do not use `error instanceof Error` at invoke call sites.
- [`src/features/player/`](../src/features/player/) — library, playlists, Discover. Discover groups tracks the UI already scanned. It has no backend of its own.
- [`src/features/search/`](../src/features/search/), [`src/features/tags/`](../src/features/tags/), [`src/features/settings/`](../src/features/settings/) — Search, Tags, and Settings (equalizer, remote, theme builder).
- [`src/features/shell/`](../src/features/shell/) — title bar, window edges, now-playing bar, speed control.

## Backend

Playback and library:

- [`src-tauri/src/player/`](../src-tauri/src/player/) — `PlayerEngine` (rodio + Symphonia), scan, queue, `play_folder_file`.
- [`src-tauri/src/eq.rs`](../src-tauri/src/eq.rs) — the only EQ. Ten biquads (peak, low shelf, high shelf). Settings → Equalizer edits that one list: tone sliders change gain, Advanced edits frequency, Q, and type. Do not add a second graphic processor.
- [`src-tauri/src/library.rs`](../src-tauri/src/library.rs) — folder roots stored in `state.json`.
- [`src-tauri/src/watch.rs`](../src-tauri/src/watch.rs) — `notify` on library and playlist folders. Failed watches are retried once a minute, not every two seconds. Past `max_user_watches` the status line explains the inotify limit. After a short quiet period it syncs playlists, relinks, and emits `library://changed`.
- [`src-tauri/src/relink.rs`](../src-tauri/src/relink.rs) — a moved file or folder is retargeted when the name is unique **inside library roots** (not `$HOME` or a root's parent). Otherwise it stays listed so the UI can ask to Locate it.
- [`src-tauri/src/playlists.rs`](../src-tauri/src/playlists.rs) — names, items, covers.

Other native work:

- [`src-tauri/src/search.rs`](../src-tauri/src/search.rs) — yt-dlp list, cache, remux, optional save.
- [`src-tauri/src/tags/`](../src-tauri/src/tags/) — read, write, batch, pictures, custom frames.
- [`src-tauri/src/mp4_recover.rs`](../src-tauri/src/mp4_recover.rs) — MP4 `mdat` size fix used by tag reads and writes. See [Tag writes](#tag-writes).
- [`src-tauri/src/persist.rs`](../src-tauri/src/persist.rs) — last folder, volume, repeat, shuffle, speed, ReplayGain, gapless, EQ (enabled, live curve, user presets), themes, playlist names and items. Old `positions` keys are ignored and dropped when that track ends. Pause does not write a resume offset.
- [`src-tauri/src/remote.rs`](../src-tauri/src/remote.rs) — LAN page. See [Web remote](#web-remote).
- [`src-tauri/src/media.rs`](../src-tauri/src/media.rs) — Linux MPRIS via souvlaki. Play, pause, seek, next, previous, and art. Desktop bars talk to this, not to the webview.
- [`src-tauri/src/desktop.rs`](../src-tauri/src/desktop.rs) — AppImage menu entry under `~/.local/share/applications`.
- [`src-tauri/src/commands/mod.rs`](../src-tauri/src/commands/mod.rs) — IPC only. The remote does not go through these commands. It calls `Player` directly.

A `state.json` that exists but does not parse is moved aside as `state.json.corrupt-<unix time>` and the app starts with defaults; the status line says so for the rest of the session. A failed save is reported the same way and cleared once a save succeeds. Nothing ever overwrites a file it could not read.

Config uses the `directories` crate: qualifier `com`, org `audios`, app `Audios` (on Linux `~/.config/audios/state.json`). Playlist pictures sit next to that file in `playlist-covers/`. Search temps are `~/.cache/audios/search/`. Playback transcodes are `~/.cache/audios/playback/`. The Tauri bundle id is `com.audios.desktop`. Those names do not match. Changing either one moves user data.

## Library and playlists

A library root is a folder. Songs added or removed on disk show up through the watcher, including after a restart.

A playlist item is a file or a folder.

- Adding a folder stores the folder. Open and play read whatever songs are there now.
- An older playlist that stored individual songs is linked back to each of those folders, so files dropped in later show up. Songs that were already in the folder and were not on the playlist stay off it.
- Removing one song writes an exclude for that file and leaves the folder linked.
- A saved file whose parent folder is still on disk, but the file is gone, is dropped. A path whose parent is missing (drive unplugged) is kept.
- Custom pictures are `playlist-covers/{id}.jpg`. If that file is missing, a 2×2 mosaic of embedded track pictures is cached as `{id}.auto.jpg`. The picker sends a filesystem path. Rust reads the image. Do not send picture bytes through IPC.

`play_folder_file` is what the remote uses when the chosen song is not already in the queue. It queues the other audio files in that folder. Above 2,000 files it queues only the chosen song.

## Web remote

The phone is a controller. Sound stays on this computer. The page is [`remote_page.html`](../src-tauri/src/remote_page.html), compiled into the binary. It is not the React app, and it does not serve yt-dlp.

`remote_start` binds each private IPv4 address (and localhost) on port 47321, or an ephemeral port if that one is taken. It does not bind `0.0.0.0`, so a VPN or public interface is not a listener. A pairing code is generated for that run. Eight wrong codes from one IP locks that IP out for ten minutes. Each request has an 8 s wall-clock deadline. The desktop shows LAN URLs and a QR code for the first one. `remote://status` updates Settings. If the accept loop dies, `stop` is set and Settings is told it is idle. Stop joins the threads and drops the song index and the cached cover.

Three threads run while it is up: accept, now-playing publish, and library index.

Every request must carry the pairing code. A miss is a 404. Routes:

| Method | Path | What it does |
| --- | --- | --- |
| GET | `/` | The phone page |
| GET | `/events` | Server-sent now-playing. Queue, folder tree, and EQ are not on this payload |
| GET | `/art` | One JPEG from `cover_for`, and only while a phone is connected |
| GET | `/search` | Filter the in-memory index. Returns ids, not paths |
| GET | `/playlists` | Playlist id, name, and song count. No paths |
| GET | `/playlist` | One playlist’s songs as index, title, and artist. No paths |
| POST | `/control` | toggle, next, previous, seek, volume, shuffle, repeat, speed, play-by-id, or playPlaylist |

The index is built from library roots and playlist songs when the remote starts, and cleared when it stops. Search play looks up the id on the server, then calls `play_folder_file`. `playPlaylist` looks up the playlist id and song index on the server, then calls `play_queue_paths` so the tapped song starts and the rest of that playlist follows. The phone never sends a filesystem path. `/playlists` and `/playlist` read the store and disk on each request, so a playlist created while the remote is running still shows up. Those responses use playlist ids and song indexes, not paths.

## Playback

Rodio keeps the output stream open and queues Symphonia decoders. Output uses the system default device. On current Linux that is PipeWire’s ALSA plugin. PulseAudio still works when it is the default.

Each file is an `EqSource` on the `audios-rodio` thread. Sample order is **decode → EQ → sink speed → sink volume**. Volume on the sink is volume × mute × ReplayGain. Speed is rodio varispeed: tempo and pitch move together. 0.5× is one octave down, 2× is one octave up. The seek bar shows file time, counted from decoded frames, so a speed change does not jump it. User seeks pass that file time through to the decoder.

Coefficients are calculated in 64-bit, and the filter recursion stays 64-bit, so a low shelf at a high sample rate does not quantize into feedback. Samples handed to the sink are 32-bit. The curve drawn in Settings is the magnitude of that same cascade. These are the minimum-phase biquads AutoEQ publishes, so a pasted profile matches that correction. Overlapping bands can sum above any single gain. Auto level uses the peak of the combined curve.

Gapless appends the next file about 1.6s before the current track ends. The queue moves when that appended source actually starts (a decoder hook), not when the sink looks almost empty. That next source has its own filter memory, so song A’s bass does not leak into song B. Seek resets the current source’s biquads. Slider changes swap coefficients on the shared EQ params and do not rebuild the sink.

EQ off, and Flat, bypass the filters. EQ never writes files or tags. User presets live in `state.json` next to custom themes, up to 20. Built-ins are compiled in. An older `gains` array loads as ten peaking bands at the old ISO centers. The reported length is the longer of the decoder duration and the tag duration. If playback passes that length, the bar grows with the overrun instead of sitting at the end.

Release builds use `panic = "unwind"` so `catch_unwind` around the decoder can turn a Symphonia panic into an error instead of killing the AppImage.

The output device can disappear (PipeWire restart, a USB DAC unplugged, suspend). Rodio's `Sink::clear` and `try_seek` wait on the audio callback and never return on a dead stream, so the engine does not call `clear`: a new song or Stop drops the sink and makes a fresh one on the same stream. Every command from the player side has a 5 s timeout. The audio thread watches for a stall (playing, sources queued, position not moving for 3 s) and rebuilds the stream. Position is restored by seeking the *decoder* before it is appended; `Sink::try_seek` is never used on a rebuilt stream, because that path waits on the callback. With no device at all, the thread retries every 5 s and on the next Play; commands answer `no audio output device` in the meantime and the player does not step to the next song.

Public `Player` operations take one outer mutex, so play/pause/seek/next cannot interleave. A poisoned lock is recovered rather than panicking the UI. The ticker and visualizer threads wrap each iteration in `catch_unwind`. Volume slider writes coalesce until the next tick (`Store::update_soon`).

Position is counted in file time from decoded frames, so changing speed does not jump the seek bar. ReplayGain may multiply volume up to 3×; the sink clamp matches that so a quiet album is not silently clipped at 1.0.

Gapless still *prepares* the next file about 1.6s before the current track ends. The hop itself is a source-started counter on the decoder, not a `queued <= 1 && position < 800` guess. Shuffle, repeat, enqueue, and seek drop a pending append if it is no longer the song that should play next. End of track skips files that will not open, then stops after one full pass; Repeat One on a missing file errors once rather than spinning.

Search play (`play_media`) takes a generation token. A second play, or any library play, invalidates the first download: the child (yt-dlp/ffmpeg/curl) is killed, and `drop_temps_except` will not delete a file whose video id is still in flight. ffmpeg is checked before the download starts. Every child process has a wall-clock timeout.

Heavy Tauri commands (scan, tags, covers, playlist play, search, remote stop) run on a blocking thread so GTK can keep painting.

Files Symphonia rejects are transcoded into `~/.cache/audios/playback/` through a `.part` file that is renamed only when ffmpeg finishes, so a half-written copy is never a cache hit. The cache is capped at 512 MB; oldest files go first, at startup and after each transcode.

## Queue order

Tracks in a playlist or folder are sorted by folder, then disc, then track number, then path. Nested folders play album-to-album in that order. That list is the playback context: the songs and the index of the one you started from.

Add to queue does not append onto that list. Those songs sit in a separate queue and play first. Next, end of track, and gapless all look there before stepping the context index forward by one. Repeat one stays on the current song and does not take from the added queue. Repeat all wraps the context only after that queue is empty. Shuffle reshuffles the context only. Playing something new replaces the context and clears the added queue.

Leaving a song forgets its place. The next play starts at 0:00. Speed is remembered.

The visualizer, when Settings turns it on, reads a decimated copy of the samples already going through the equalizer. A 256-point magnitude transform runs about 30 times a second on its own thread and sends 32 levels to the webview. The canvas draws them. The decode callback does not run that transform, and it skips the copy entirely while the visualizer is off.

## Tag writes

Batch writes continue past a file that fails and return how many were written plus each failure. The original file is copied to a sibling temp that **keeps the audio extension** (`.song.audios-tmp.mp3`, not `.song.mp3.audios-tmp`). A staging file older than an hour is a leftover from a crash; the scanner and the next write in that folder remove it. Lofty’s `read_from_path` decides the format from the extension. A `.audios-tmp` suffix makes it report that no format could be determined. A failed write deletes the temp and leaves the original untouched. Do not write tags in place. Not every container has the same frames. Artwork is sniffed from magic bytes and kept as JPEG or PNG (or converted to JPEG) so a packed WebKit `File.type` of `""` does not label a PNG as JPEG. `read_tags` lists pictures without their bytes. Tags loads a downscaled JPEG per picture through `picture_preview`. Adding artwork sends a filesystem path; Rust reads the image. Cover art sent to the webview is capped at 512 px on the long edge.

MP4-family files (`.m4a`, `.mp4`, and the same container under other names) sometimes store `mdat` with a 64-bit size. Lofty 0.22 then skips eight bytes past that atom and reports that `moov` is missing. When that size still fits in 32 bits, [`mp4_recover.rs`](../src-tauri/src/mp4_recover.rs) rewrites the header to a normal 8-byte atom in memory for reads, and on the staging file for saves. Files larger than 48 MB are skipped so a 600 MB audiobook does not double RSS. Audio samples are not rewritten. Chunk offsets move back by those eight bytes. The original is replaced only if the tag write succeeds.

## Search

Two steps. The UI already has the title from step 1. Step 2 is not a second text search.

1. **List** — `yt-dlp -J --flat-playlist`. A text search runs `ytsearchN:` first, then `scsearchN:` for SoundCloud. A pasted link is passed through, so Bandcamp and other yt-dlp sites work when the URL is known. This step is metadata only. Cover search in Tags stays on YouTube thumbnails and does not download audio. A SoundCloud hit keeps its `webpage_url`. Do not turn that id into a YouTube watch URL.
2. **Play** — `cache_media(watch_url)` downloads a temp file, remuxes it, then `play_tracks` loads that path. Switching tracks deletes other files in the search cache. **Download** copies the original cached file and keeps the extension the user picked.

### Workarounds

Do not revert these without a failing file in hand.

| What | Why it is there |
| --- | --- |
| Newest yt-dlp on disk, not the first `PATH` hit | `apt` yt-dlp is often years old and cannot extract current YouTube player responses. Prefer `~/.local/bin` when that copy is newer. Override with `AUDIOS_YTDLP`. |
| `--extractor-args youtube:player_client=mediaconnect` | `android`, `ios`, `tv`, and `web` often return “requested format is not available”. mediaconnect still yields AAC. Send it only for YouTube URLs. SoundCloud and other sites use yt-dlp’s defaults. |
| `--js-runtimes node` when `node` exists | Helps yt-dlp solve YouTube JS challenges. |
| No `--print filename` and no `--simulate` on download | Those flags skip writing the file. |
| Remux YouTube AAC `.m4a` to mp3 or wav via ffmpeg | rodio 0.20 + Symphonia panics (`Seek errors should not occur during initialization`) on these MP4s. Library `.m4a` files can hit the same bug. |
| `catch_unwind` around `Decoder::new` | The panic is inside rodio, not a `Result`. |
| A library file that fails `Decoder::new` is transcoded into `~/.cache/audios/playback/` | Search remux deletes its source. A library file must stay where it is. ffmpeg writes an mp3, then wav, and playback uses that copy. |
| Invidious / Piped HTTP fallbacks | Last resort if the yt-dlp download fails. Public instances go stale. Treat the host list as disposable. |
| Sanitize child env for yt-dlp, ffmpeg, and curl. Skip AppImage `APPDIR` on `PATH` | AppImage AppRun points `PYTHONHOME`, `LD_LIBRARY_PATH`, `GIO_EXTRA_MODULES`, `GCONV_PATH`, and other vars at `/tmp/.mount_*`. Host Python then dies with `Python path configuration:`. Named poison vars are dropped, plus any env whose value lives under the mount. Child processes go through `spawn_tool`. |

Search needs a current yt-dlp, ffmpeg, and curl. Spotify is not a source.

## UI notes

- The window is frameless. Minimize, maximize, and close sit on the top right. The UI fills the client area. `html` and `body` use `--app` so compositor square chrome matches the page. Do not add inset frame padding. Columns and lists use the window width and wrap. Do not pin a page to a fixed narrow measure.
- The full now-playing view sets its own `--app` and `--app-accent` from the album art. Those variables stay on that overlay. The rest of the app keeps the saved theme.
- Stay on WebKit-safe CSS.
- The footer status line is red for errors and muted for info (`Saved tags`, `Getting audio…`, Vite preview).
- Player, Search, Tags, and Settings stay mounted and toggle with `hidden`, so tab state survives a switch.
- Each tab, the full now-playing view, and everything under the title bar sit inside an `ErrorBoundary` (`src/ui/ErrorBoundary.tsx`). A render error shows a message with Try again and Reload; the window buttons keep working.
- One-way actions (delete playlist, remove a picture, field, EQ preset, or theme, and Apply to N in Tags) go through `confirm()` from `src/ui/confirm.ts`, rendered by `ConfirmHost`. Cancel has focus; Escape cancels. Apply to N lists which fields will be set and which are blank and will be cleared.

## Security

- `app.security.csp` is `null`. A tighter CSP breaks `data:` cover art and local asset loads unless those sources are listed.
- Search shells out to yt-dlp, ffmpeg, and curl. Queries are length-limited. URLs are arguments, not a shell string.
- Library walks use `follow_links(true)`. WalkDir skips symlink cycles. A symlink farm can still make a scan huge.
- The remote binds private IPv4 addresses and localhost after Start web remote, not `0.0.0.0`. A request without the pairing code is a 404. Eight failed codes from one IP lock that IP out for ten minutes. Stopping it drops the song index and the cached cover. It does not expose YouTube search or file paths.

## Known issues

- **YouTube extractor drift.** mediaconnect, format IDs, and public frontends will rot. Fix the extractor args or the host list. Do not add a stricter `-f bestaudio[ext=m4a]` selector.
- **Library decode.** `.m4a` and other containers are still scanned as playable. If Symphonia cannot open one, playback uses the ffmpeg cache copy. The original file is not rewritten.
- **Search cache** is one file per video id. A failed remux must not leave only an unplayable `.m4a` as the cache hit. `prepare_for_player` remuxes that path again. Save copies the original download and keeps the extension the user picked.
- Search unit tests are JSON and path checks. They do not hit live YouTube, and they do not require yt-dlp or ffmpeg.

## Checks

```bash
npm run check        # typecheck + eslint + prettier --check + vitest
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
```

CI runs the same set on every push and pull request (`.github/workflows/ci.yml`). `npm run format` rewrites files to the Prettier style.

Frontend tests run under Node by default. A file named `*.dom.test.tsx` gets jsdom and Testing Library, for component tests.

`Player` can be driven without audio or a window. `player::fake_engine::FakeEngine` stands in for rodio and `RecordingHost` collects events; `Player::with_parts` assembles one with no background threads, and tests call `player.tick()` themselves to step end-of-track and gapless logic.
