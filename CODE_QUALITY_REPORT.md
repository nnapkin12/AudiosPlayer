# Audios! — Code Quality & UX Report

Read-only audit of the repository at commit `75e68ac` (v0.4.0). No files were modified.
Method: full read of the playback core, store, app shell, and persistence by the lead reviewer; three parallel deep audits (Rust playback, Rust services, React/UX); `tsc --noEmit` clean; `vitest` 38/38 pass; `cargo test --no-run` compiles with one dead-code warning; rodio 0.20.1 source consulted for the engine findings.

---

## Executive verdict

**Is it good?** Honestly: better than most AI-generated codebases, and better than a lot of hobbyist FOSS. It is not yet "consumer-ready".

What is genuinely strong:

- The architecture is right. Disk, audio, child processes and tags live in Rust; the webview only renders and `invoke`s. There is a single IPC boundary (`src/lib/api.ts`). This is the correct shape for a Tauri app and it will scale.
- `docs/architecture.md` is accurate, specific, and explains *why* (the yt-dlp workaround table is exemplary). Most projects never get this.
- Security basics are done properly where it matters: no shell strings anywhere (args + `--`), timing-safe pairing-code compare, constant-404 on auth failure, zero `innerHTML` in the phone page, request size limits, tag writes are copy-edit-rename, `state.json` is write-then-rename.
- The pure logic is well tested and solid: `Queue` (10 tests, bounds-safe everywhere traced), EQ numerics (16 tests incl. 192 kHz shelf stability, NaN/Q=0/Nyquist all handled), `catalog.ts`, `eq.ts`, `theme.ts`.
- Frontend performance discipline is real: narrow Zustand selectors, `player://tick` only touches `positionMs/durationMs`, generation tickets on async loads, LRU + semaphore on cover art, DPR-aware visualizer, `prefers-reduced-motion` honored.
- Zero `any` in TypeScript. Strict mode with `noUnusedLocals/Parameters`.

What holds it back (the Grok signature):

- **The pure functions are tested; the state machines are not.** `Player` (the thing that actually plays music), the audio engine, the watcher, the HTTP parser, and every React component have zero tests. Every High-severity bug below lives in that untested seam.
- **Happy-path engineering.** Blocking calls with no timeout, no retry when the audio device is missing, no confirm on destructive actions, no error boundary, corrupt state file silently wiped, caches that grow forever.
- **Local-lock-window concurrency.** Each operation takes the mutex several times briefly with blocking I/O (ffmpeg, disk, decoder probe) in between. Correct in isolation, racy under the 250 ms ticker + MPRIS thread-per-event + IPC pool.
- **Copy-paste instead of abstraction.** Four copies of `ProjectDirs::from(...)`, three JPEG encoders, three tool-path resolvers, two `Toggle` components, two `nextRepeat`, two search fields, the same Play-button className five times.
- **No lint, no format, no CI.** No ESLint, no Prettier, no `.github/`. Several bugs below would have been flagged by `eslint-plugin-react-hooks`.

---

## 1. State machine & logic gap audit

Severity: **Critical** = freezes/bricks the app or loses data. **High** = wrong behaviour a normal user will hit. **Medium** = wrong behaviour under plausible edge conditions. **Low** = latent.

### Playback core (`src-tauri/src/player/`)

**[CRITICAL] P1. Audio-thread deadlock propagates to the whole app.**
`RodioEngine::send` blocks on `rx.recv()` with no timeout (`engine.rs:88`). On the audio thread, `SetUri` and `Stop` call `sink.clear()` (`engine.rs:237, 280`) and `Seek` calls `sink.try_seek()` (`:244`). In rodio 0.20.1, `Sink::clear()` → `sleep_until_end()` → `recv()` (verified: `rodio/src/sink.rs:290-295, 324-328`), and `try_seek` waits on `feedback.recv()` — both are only satisfied by the periodic-access closure running inside the cpal output callback. If the output stream dies (PipeWire restart, USB DAC/Bluetooth unplugged, suspend/resume), cpal only prints to stderr and the callback stops. The next load/stop/seek hangs the audio thread; every Tauri command, the ticker (`mod.rs:139-143`), MPRIS dispatch and the web remote then block inside `send`. **Whole-window freeze with a frameless window and no way to close except the OS.** Also: `OutputStream::try_default()` failing at startup (autostart before PipeWire is up) leaves the engine permanently returning `"no audio output device"` (`engine.rs:199-204`) with no retry. `architecture.md:109` says "Rodio keeps the output stream open"; nothing detects it closing.

**[HIGH] P2. `Player` operations are not serialized; the ticker races commands.**
Each public op is a *sequence* of short `logic` lock windows with blocking I/O between them. Three real interleavings:
- `next()` (`mod.rs:278-295`) advances the queue, drops the lock, then runs `convert::for_playback` (decoder probe, or **ffmpeg for seconds**) before `set_uri`. If the old track ends in that window, `tick()` sees `want_playing && is_empty()` (`:818`) → `on_eos()` → `advance()` again. Queue index at N+2, sink playing N+1.
- `maybe_queue_gapless` (`:745-765`) reads `pending=false`, drops the lock, runs `for_playback`, then `set_gapless_next`, then sets `pending=true`. A user `next()` in between clears the sink and resets `pending`; the late `Append` lands *after* the new track.
- MPRIS spawns **a thread per event** (`media.rs:42-45`); UI click + headset button both call `next()` → double skip.

**[HIGH] P3. Gapless pending state is never invalidated on mode changes.**
Once the next source is appended to the sink, `set_repeat`, `set_shuffle`, `enqueue_path` and `seek` (`mod.rs:316-321, 479-499, 522-541`) do not touch `pending_gapless` or the sink. `on_gapless_started` (`:767-781`) then calls `queue.advance()`, which under a fresh shuffle order returns a *different* index, and with a freshly enqueued song returns the *user-queue* track — while the sink plays the originally appended context track. UI, MPRIS and the phone all show a song that is not the one you hear. `on_eos` also has two identical branches (`:787-792`), so if the 800 ms detection window (`:823`; only ~400 ms wall-time at 2× speed) is missed, track B is loaded and **played twice**.

**[HIGH] P4. Stale duration and sample rate after a gapless hop.**
`load_into(.., replace=false)` (`engine.rs:279-288`) only updates `duration_ms` and `sample_rate` in the `replace` branch. After a gapless transition, `Shared.duration_ms` is still track A's. `current_duration` takes `max(decoded, tagged)` (`mod.rs:718`), so when B is shorter than A the bar is wrong for all of B and the next preroll (`position + 1600 >= duration`) fires late or never — **the gapless chain silently breaks after one hop.** The EQ curve is also drawn against A's sample rate.

**[HIGH] P5. Repeat-one on a broken file spins a 4 Hz error loop with disk writes.**
`on_eos` (`:783-804`): load fails → `error` set, but `want_playing` stays true and the sink is empty, so the next tick calls `on_eos` again. With `RepeatMode::One`, `advance()` returns the same broken track forever: every 250 ms it runs `forget_current` → `persist.update` → **full `state.json` rewrite**, and `for_playback` → for an undecodable `.m4a`, **spawns ffmpeg every 250 ms**. Same for a queue whose files are all gone with repeat-all (unplugged drive).

**[HIGH] P6. Play and Previous do nothing after the queue ends.**
`on_eos` with no next track pauses and leaves the sink empty (`:798-801`); the queue index stays on the last track. `play()` (`:238-243`) just `sink.play()`s an empty sink and sets `want_playing=true`, so the next tick → `on_eos` → `advance()` → `None` → pause. `previous()` reads `position_ms() > 3000` from the last published position (end of track) → `seek(0)` on an empty sink → no-op. The user sees a track with a Play button that does nothing. Same mechanism: `stop()` then `play()` skips a track.

**[MEDIUM] P7. Position readout jumps when speed changes mid-track.**
`position_ms = wall × speed` (`engine.rs:139-142`) is only correct if speed was constant since load. Play 60 s at 1×, switch to 2× → slider reads ~120 s, the overrun logic (`mod.rs:724-728`) inflates duration, gapless preroll fires early.

**[MEDIUM] P8. Mutex poisoning kills the ticker permanently.**
~40 `lock().expect("player lock")` sites. The ticker thread (`mod.rs:137-143`) and viz thread have no `catch_unwind`; a poisoned lock kills them and EOS/gapless detection stops for the session. `EqShared` does this right (`unwrap_or_else(|e| e.into_inner())`, `eq.rs:990-1000`); `Player` should too.

**[MEDIUM] P9. `catch_unwind` covers only `Decoder::new`.**
Panics *during* decoding happen in `EqSource::next` on the cpal callback thread; nothing catches them. The stream goes silent and the engine still reports `playing`.

**[MEDIUM] P10. Latent panic on pasted AutoEQ text.** `line[..9]` (`eq.rs:723`) slices a `&str` by bytes; five two-byte characters panic on a non-char boundary.

**[LOW]** `play_index` clears the user queue (`queue.rs:90`) even within the same context — doc `:125` only promises that for "something new". `jump_to_folder` uses `starts_with` without a separator (`queue.rs:184`): `/m/Al` matches `/m/Album/…`. `snapshot.index` points at the context track while `current` is a user-queue track. Volume clamp is 1.5 on the sink (`engine.rs:250`), 1.0 in `Player` (`mod.rs:504`), and ReplayGain can request 3.0 (`scan.rs:305`) — RG boost silently clipped. `try_seek` on an empty sink leaves the seek order pending and rodio applies it to the *next* appended source.

### Services (`search.rs`, `remote.rs`, `persist.rs`, `commands/`)

**[HIGH] S1. Corrupt or incompatible `state.json` silently wipes all user data.**
`Store::load` does `serde_json::from_str(&raw).ok().unwrap_or_default()` (`persist.rs:173-176`). A truncated file, a hand edit, or a type change on any of the seven v1 fields that lack `#[serde(default)]` (`volume, muted, repeat, shuffle, replaygain, gapless, positions`, `:34-42`) resets **playlists, custom themes, EQ presets and library roots** — then the very next `update()` overwrites the file. No `.bak`, no error surfaced. `update()` also discards write errors: `let _ = save_to(...)` (`:207`) — disk full = silent loss.

**[HIGH] S2. Stale-track race in search playback — no cancellation.**
`play_media` runs `cache_media` on a blocking thread then `player.play_tracks` (`commands/mod.rs:503-508`). Tap B while A is still downloading → A finishes later and *replaces B*. No generation counter, child process never killed. `drop_temps_except(Some(B))` (`player/mod.rs:684`) can also delete A's freshly remuxed file while `cache_media(A)` is returning that path.

**[HIGH] S3. Heavy sync Tauri commands run on the main GTK thread.**
Non-`async` commands execute on the main thread. `batch_write` (`commands/mod.rs:220`), `scan_tracks` (`:109`), `playlist_cover` (decodes up to 48 embedded images for the mosaic, `playlists.rs:288-295`), `write_tags`, `cover_art`, `remote_stop` (joins threads), `play_playlist` (walks folders) are all sync. Only the six search commands use `run_blocking` (`:522`). A 500-file batch tag write freezes the window.

**[HIGH] S4. Remotely reachable panic in the auth path.**
`percent_decode` slices a `&str` by byte offset: `&raw[index + 1..index + 3]` (`remote.rs:1329`). A query like `?code=%€` panics before the pairing check. `panic = "unwind"` confines it to the client thread, but it is a panic any device on the LAN can trigger.

**[MEDIUM] S5. `accept_loop` dies silently** on any error other than `Interrupted` (`remote.rs:705`) — Settings keeps showing "running" with a dead listener. `MAX_CONNS` is load-then-`fetch_add` (`:835-844`), not atomic.

**[MEDIUM] S6. Remote bind and rate limiting.** Binds `0.0.0.0` (`:684-685`) while `lan_urls` shows only RFC1918 addresses; on a laptop with a VPN or public interface the socket is reachable there too. Six-character code, **no rate limit or lockout**. Plain HTTP with the code in the query string (browser history, LAN sniffing). The 8 s timeout is per `read()`, not per request (`:846`) — 24 trickling clients exhaust the pool. Design is defensible for a QR-paired LAN remote; the docs should state the threat model.

**[MEDIUM] S7. Child processes have no wall-clock timeout.** All `.output()` calls block indefinitely (`search.rs:121, 703, 1198-1199`). `--socket-timeout 15` does not cover a hung `--js-runtimes node` challenge.

**[MEDIUM] S8. `batch_write` aborts on first error and loses the count** (`tags/mod.rs:182-195`): files 1..k are rewritten, the caller gets only an error string.

**[MEDIUM] S9. `mp4_recover` loads the whole file into memory twice** (`mp4_recover.rs:62, 80`). A 600 MB `.m4b` → 1.2 GB RSS. `stco`/`co64` shifting is correct; fragmented MP4 (`moof`/`tfhd`/`trun`/`sidx`) offsets are *not* patched even though `moof`/`traf` are listed in `is_container` (`:195-196`).

**[MEDIUM] S10. `save_media` delivers a re-encode, not the original**, and overrides the user's chosen extension (`search.rs:220-228`). "Download" gives lossy-of-lossy MP3 or a 10 MB/min WAV.

**[LOW]** `csp: null` (`tauri.conf.json:29`) plus commands that read/write arbitrary paths (`export_picture`, `save_media`, `set_playlist_cover`, `add_library_root`) means one XSS from an untrusted tag string is arbitrary file I/O; CSP is the second wall and it is down. Two concurrent tag writes to one file share a staging name and clobber (`tags/mod.rs:415-418`). `add_picture(data: Vec<u8>)` (`commands/mod.rs:230`) ships a 5 MB cover as a ~20 MB JSON array, contradicting `architecture.md:82`. `HOME` unset → `expect` panic in three places.

### Frontend (`src/`)

**[CRITICAL] F1. No error boundary anywhere.** Zero `ErrorBoundary`/`getDerivedStateFromError` in `src/`. A single render throw (malformed snapshot, `snapshot.current!.path` at `TagsView.tsx:296`) blanks the whole frameless window — the close button is React too.

**[CRITICAL] F2. Destructive actions have no confirmation and no error handling.**
- Delete playlist: one click in a right-click menu (`Playlists.tsx:399` → `remove()` `:347-354`; also `PlayerView.tsx:517-521`), `await api.deletePlaylist` with **no try/catch**, no undo.
- **"Apply to N"** batch tag write (`TagsView.tsx:604-612`): line `:276-277` warns "Empty fields clear any existing tags", yet `saveCurrent` (`:216`) sends every field including blanks. One click can strip tags from a whole folder. No confirm, no dry-run, no undo.
- Remove embedded picture (`TagsView.tsx:514-523`), remove custom field (`:585`), delete EQ preset (`EqPanel.tsx:259`), delete custom theme (`SettingsView.tsx:161`) — no confirm; first three have no `.catch`.
- `addFiles/addFolder` (`Playlists.tsx:298-310`), `pickAudioFilesInto/pickFolderInto` (`PlayerView.tsx:524-542`), `addToPlaylist(...).then(...)` (`:188-194`) — unhandled rejections.

**[HIGH] F3. Cover-art race in the store.** `refreshCover` (`useAppStore.ts:149-162`) awaits `api.coverArt(path)` and unconditionally `set({ coverUrl })`. Skip tracks quickly with Now Playing open → an older, slower cover lands last, and `useArtPalette` recolors the whole overlay from the wrong album.

**[HIGH] F4. Seek bar fires an IPC per pixel and snaps back.** `SeekBar.tsx:66-71` calls `onSeek` on every `onChange` during drag — dozens of `seek` invokes per second. On release, `drag` is nulled (`:24-27`) and the thumb reverts to the extrapolated pre-seek position (`motion.ts:27-30`) until the next tick → visible snap-back after every drag.

**[HIGH] F5. Transport error handling is inconsistent and mostly absent.** `NowPlayingFull.tsx:67-110` uses `void api.toggle()` etc. with no `.catch` (unhandled rejections); `NowPlayingBar.tsx:70-143` does `.catch(() => undefined)` and swallows everything — a dead audio device produces *no* status message. No in-flight guard anywhere (`TrackList.tsx:47-57`, `PlayerView.tsx:87-112`): double-click = two `play_tracks`. Five call sites do `applySnapshot(await api.toggle())` while the command *also* emits `player://state`, so every click applies the snapshot twice.

**[HIGH] F6. `applySnapshot` clobbers the status line.** `useAppStore.ts:130-136` sets `status: snapshot.error, statusTone: "error"` on **every** state event. "Saved tags" / "Getting audio…" vanish the moment any state event arrives (volume tick, track end). Conversely an error is sticky with no dismiss or timeout.

**[HIGH] F7. Global keyboard handler hijacks focused controls.** `App.tsx:116-136`: `Space` `preventDefault`s and toggles playback even when a `<button>` has focus — keyboard users cannot activate Play/Next/menu buttons with Space once a track is loaded (WebKit activates on keyup, so it can also double-fire). `ArrowLeft/Right` seek ±5 s while a `<select>` is focused; single-letter `n`/`p`/`f` fire on any non-input element. No shortcut help anywhere.

**[MEDIUM] F8. Effect cleanup gap in `App.tsx`.** `stop.push(await listen(...))` (`:74-76, 101`) never re-checks `disposed` after the `await`. Under StrictMode (on, `main.tsx:10`) the first run's listeners register *after* cleanup → duplicate `applySnapshot`/`refreshFromDisk` in dev. `TagsView.tsx:170-210` does it correctly — inconsistency, not ignorance.

**[MEDIUM] F9. Window focus triggers a full rescan.** `App.tsx:104-113` registers both `focus` and `visibilitychange` → `refreshFromDisk` → `bumpLibrary()` → `FolderArt` (`Playlists.tsx:171-183`) re-scans **every library root** for four thumbnails. Alt-tab back = double-fired rescan.

**[MEDIUM] F10. EQ frequency input is untypeable.** `remote = snapshot?.eq` is a fresh object on every `applySnapshot` (`useAppStore.ts:121-129`), so the `[remote]` effect (`EqPanel.tsx:58-71`) rewrites local state on every state event. Typing `5` → clamped to `20` → pushed → snapshot → input shows `20`; you cannot type `500`.

**[MEDIUM] F11. `VirtualList` does not clamp to `items`** (`virtualList.tsx:38-41`): scroll deep, then filter → `start > items.length` → empty list until a corrective scroll event. Rows unmount on scroll so keyboard focus is lost; no `role="list"`.

**[MEDIUM] F12. Discover cache never invalidates on tag edits.** `discoverTracks` (`DiscoverView.tsx:23-24`) is keyed only by roots + playlist ids; `applyTrackMeta` (`browse.ts:32-41`) patches `pageTracks` and `browseCache` but not this. Fix an artist tag → Discover shows the old artist until restart. Artist lookup at `:100-102` uses `toLocaleLowerCase()` while `catalog.ts:19-26` uses `artistKey()` — inconsistent normalization.

**[MEDIUM] F13. Double submit on rename** (`PlayerView.tsx:340-349`, `Playlists.tsx:359-372`): Enter → `onSubmit` → `commitRename`; then blur → `commitRename` again before the first resolves.

**[MEDIUM] F14. Visualizer resets on pause** (`Visualizer.tsx:116` deps include `playing`) — bars vanish instead of decaying; stale canvas scale on resize while paused (no ResizeObserver).

**[LOW]** `TagsView.tsx:228-238` Ctrl+S effect has no dependency array and detects visibility via `document.querySelector("[data-tags-root]")?.closest(".hidden")` — coupled to `App.tsx`'s class name. Duplicate keys at `TagsView.tsx:575` for repeated identical raw frames. `refreshFromDisk` handles a removed folder root but not an externally deleted playlist. `useSmoothPosition` calls `matchMedia` every render (`motion.ts:51`).

---

## 2. Edge-case & Linux UX review

### Layout and window

**[CRITICAL] U1. Layout breaks at the declared 480×320 minimum.** `tauri.conf.json` says `minWidth: 480, minHeight: 320`; the code cannot render there.
- Width: Sidebar `w-16` (`Sidebar.tsx:10`) + `.player-nav { width: 220px }` (`styles.css:227-229`) = 284 px → **196 px** for content. `.cover-grid` `minmax(240px,1fr)` (`:233`) overflows it. Tags: 64 + `w-[280px]` file pane (`TagsView.tsx:281`) → **136 px** for the editor.
- Height: titlebar 40 + bar `h-[92px]` (`NowPlayingBar.tsx:34`) → 188 px for content. `NowPlayingFull` stacks cover + three margins + controls inside `flex items-center` with **no `overflow-auto`** (`NowPlayingFull.tsx:37`) → clipped top and bottom; the close button is unreachable.
- `architecture.md` "UI notes" says "Do not pin a page to a fixed narrow measure." The code violates its own rule.

**[HIGH] U2. The app does not open files.** `desktop.rs` registers `Exec="…" %U` and a `MimeType=` line for every audio type, so Audios! appears in every file manager's "Open with" — but **nothing reads `std::env::args`** (zero matches in `src-tauri/`), and there is no single-instance plugin. Double-clicking an MP3 launches a second, empty Audios!. For a "Linux music player" this is the first thing a new user tries. Also: the icon PNG is rewritten on every launch (`desktop.rs:26`), `%`/`\`/`$` in the AppImage path are not escaped (`:38`), `update-desktop-database` is never called, and deleting the AppImage leaves a dangling `.desktop` + icon with no uninstall path.

**[HIGH] U3. No volume, EQ or speed before first play.** `App.tsx:176` hides `NowPlayingBar` when `!hasTrack`. A fresh install has no transport row at all.

### Discoverability and input

**[HIGH] U4. Right-click is the only path to most actions.** Playlist cards (`Playlists.tsx:378`), folder cards (`:149`) and the playlist cover (`PlayerView.tsx:303`) expose Rename / Delete / Add / Change picture / Locate **only** via `onContextMenu`. Touchpad users without a configured secondary click, and every keyboard user, never find them. `TrackList` has a `…` button (`TrackList.tsx:113-121`); the cards should too.

**[HIGH] U5. Context menu is mouse-only.** `ContextMenu.tsx`: no `role="menu"`, no arrow-key navigation, no initial focus, submenus open on `onMouseEnter` only (`:121`). Position math assumes 220 px × 32 px rows (`:53-54`) and ignores separators.

**[MEDIUM] U6. Accessibility baseline is missing.** ~95 `<button>` elements, ~28 `aria-*` attributes total. Icon-only transport buttons in `NowPlayingFull.tsx:77,85,101,109` have `title` but no `aria-label` (while `NowPlayingBar.IconButton` does it right). `outline: none` on range inputs (`styles.css:150`) with no `focus-visible` replacement; no custom focus ring anywhere. Status line (`App.tsx:164-172`) is a bare `<p>` — no `role="status"`/`aria-live`, not dismissible. `ThemeBuilder` has `role="dialog"` but no focus trap; `NowPlayingFull` overlay has neither.

**[MEDIUM] U7. Drag-and-drop is nearly absent.** Works only in Tags with no document loaded (`TagsView.tsx:171`). Dropping an album on the Player tab does nothing. The HTML5 `onDrop` branch reads `(file as File & { path?: string }).path` (`:362`) — `File.path` is Electron-only; dead code in WebKitGTK.

**[MEDIUM] U8. Missing loading / empty / error states.** Search hides its hint while loading (`SearchView.tsx:152`) and shows only a disabled button reading "Searching" → blank panel. `TrackList` has no skeleton. Library scans of large trees (`collect_tracks` reads tags for every file synchronously) have no progress. `RemotePanel` with no private IP gets `running: true, url: None` (`remote.rs:298-302`) — check what the UI shows. Phone page: `send()` swallows all failures (`remote_page.html:560, 564`), `runSearch` maps every error to "No songs match" (`:621-623`), and `EventSource` retries forever against a 404 after the desktop restarts the remote with a new code — the phone just looks frozen.

### Resources and memory

**[HIGH] U9. Playback transcode cache grows forever.** `~/.cache/audios/playback/` (`convert.rs:57-68`) is keyed by (path, mtime, len). Re-tagging a file changes mtime → new entry, old one orphaned. MP3 ≈ 1 MB/min, WAV fallback ≈ 10 MB/min. **Nothing ever deletes anything.** (Search cache *is* bounded — good.) Truncated transcodes are also accepted as valid: ffmpeg writes straight to `dest` (`:102-110`), `fresh_cache` accepts any `len > 0` (`:29-35`); kill the app mid-transcode and the truncated file plays forever.

**[MEDIUM] U10. Disk write per UI event.** `set_volume`, `set_muted`, `forget_current` each do a full JSON serialize + write + rename under the persist mutex (`persist.rs:205-207`). A volume drag = dozens of `state.json` rewrites per second. Every track change writes to remove a `positions` key for a feature `architecture.md:63` says is dead.

**[MEDIUM] U11. Frontend memory.** `PICTURE_CAP = 48` full-resolution `cover_art` data-URLs (`covers.tsx:7`) can be 50–100 MB of base64. `TagDoc.pictures[].dataBase64` ships every embedded picture on each `readTags`. `addPicture` sends `Array.from(new Uint8Array(...))` (`TagsView.tsx:258`) — 5 MB → ~20 MB JSON.

**[MEDIUM] U12. inotify and big libraries.** `RecursiveMode::Recursive` (`watch.rs:56`) needs one watch per directory; past `max_user_watches` the `watch()` fails and is silently dropped (`.is_ok()`), and because `current != wanted` every loop, **the whole tree is re-walked and re-attempted every 2 s** (`:33`). `dirs_to_watch` also `stat`s every playlist item every 2 s. `Err`/`Rescan` events are discarded (`:22-24`).

**[MEDIUM] U13. `relink` walks `$HOME`.** `search_places` adds every root's *parent* and every item's *grandparent* (`relink.rs:260-278`); `index_places` walks them 5 deep with `follow_links(true)` (`:295`). One missing file → a 5-deep walk of the home directory on each watcher event and at startup. `relink_playlist` runs `audio_paths(dest)` *inside* `store.update` (`:145-157`), holding the persist mutex across disk I/O. `retarget_library` repoints a missing root to any unique lowercase basename match (`:82-95`) — a root named `Music` can silently become `~/Videos/music`.

**[MEDIUM] U14. Blocking work on the ticker.** `maybe_queue_gapless` runs `for_playback` (possibly ffmpeg) inside `tick()` → no ticks, no EOS detection, frozen progress bar for the duration.

**[LOW]** Non-UTF-8 filenames become `U+FFFD` in `Track.path` (`scan.rs:162`) → "That song is missing" with no way to play. `.audios-tmp.*` staging files survive a crash and are never swept. XDG: playlist covers and artist images are *data* stored under `config_dir` (`playlists.rs:143, 187`); doc says `~/.config/audios/Audios/state.json` but `directories` yields `~/.config/audios/state.json`. `-webkit-app-region: drag` (`styles.css:168`) is a no-op in WebKitGTK; only `data-tauri-drag-region` works. `user-select: none` on `body` (`:113`) blocks copying the file path shown in Tags. Not RTL-ready (physical classes throughout). `run_ytdlp` spawns `node --version` on **every** call (`search.rs:1194`) and `find_ytdlp` runs `--version` on every candidate every call (`:1245`) — one text search ≈ 10+ extra process spawns. MPRIS: thread-per-event unbounded (`media.rs:42-45`); `write_cover` deletes other covers while the shell may still be loading them (`:264-277`).

---

## 3. Code smells & redundancy

### Duplicated code

Rust:
- `ProjectDirs::from("com","audios","Audios").expect(..)` ×4: `search.rs:1288`, `persist.rs:239`, `player/convert.rs:63`, `media.rs:254`. → one `paths.rs`.
- `DefaultHasher` hex-id helper ×4: `search.rs:788-793`, `playlists.rs:140-146, 302-306`, `convert.rs:46-52`.
- ffmpeg remux loop (identical args + run/verify/cleanup): `search.rs:685-714` vs `convert.rs:81-119`.
- Three JPEG pipelines at three qualities: `tags/mod.rs:358-384` (q80), `:985-999` (q90), `playlists.rs:226-257` (q88).
- Three tool-path resolvers: `find_tool` (`search.rs:729`), `find_ytdlp` (`:1208`), `host_path` (`:1162`).
- Three "title or stem / artist or album_artist" fallbacks in `remote.rs` (`:802-816`, `:1178-1194`, `:1268-1281`).
- 404 `write_text` block ×4 in `handle_client` (`remote.rs:851, 873, 882`). PNG sniff at `remote.rs:1260` duplicates `tags::sniff_image_mime`.
- `play_queue_paths` ≈ `play_tracks` (`player/mod.rs:417-475`); `on_eos` identical branches (`:787-792`); auto-preamp computed twice with **different sample rates** (`eq.rs:536-545` at 48 k vs `:1147-1156` at real rate) so UI and DSP disagree; `decoder_for` builds the same error string twice (`engine.rs:301-317`); `snapshot_eq` and `transport` gather the same fields; `Track` literal repeated in three test modules.

TypeScript:
- `Toggle`: `EqPanel.tsx:561-586` vs `SettingsView.tsx:290-315`.
- `nextRepeat`: `NowPlayingBar.tsx:155-159` vs `NowPlayingFull.tsx:177-181`.
- Color row: `SettingsView.VizColor:339-379` vs `ThemeBuilder.ColorField:139-167`.
- Search field: `PlayerView.ListSearch:482-515` vs `DiscoverView.tsx:218-239` (identical markup).
- Transport row: `NowPlayingBar.BarTransport:100-152` vs `NowPlayingFull.tsx:74-119`.
- `CoverPicture`, `CoverThumb`, `PlaylistCover` (`covers.tsx:145-233, 270-316`) are one component with three caches.
- Playlist mutations (`changeCover/removeCover/commitRename/addFiles/addFolder`) in both `PlayerView.tsx:114-150, 524-542` and `Playlists.tsx:298-345`.
- Audio extension list twice: `api.ts:52-64` and `:83-96`.
- The primary Play-button className string ×5 (`PlayerView:398,420`, `DiscoverView:429,518`, `Playlists:130`).
- Three sources of truth for theme colors: CSS vars (`styles.css:5-73`), `theme.ts:colorsForBuiltin:228-280`, `DEFAULT_THEME_COLORS:77-94`. Visualizer defaults `#8ec8ff/#e8f3ff/#4aa3ff` in `App.tsx:64-66`, `useAppStore.ts:84-86`, `SettingsView.tsx:358`, and `persist.rs:105-115`.

### Files that need splitting

- `eq.rs` (1652) → `eq/mod.rs` (types), `eq/persist.rs` (Raw structs, legacy `gains`), `eq/presets.rs` (builtins, `tone_bands`, `MACRO_LOCKS`, upsert/delete), `eq/dsp.rs` (`BiquadCoeffs`, `design_coeffs`, `Biquad`, `EqFilter`), `eq/source.rs` (`EqShared`, `EqSource`), `eq/parametric.rs` (AutoEQ parser), `eq/analysis.rs` (`magnitude_db`, `composite_peak_db`).
- `search.rs` (1585) → `search/tools.rs` (find_*, spawn_tool, env strip), `search/ytdlp.rs` (run/json/probe/pick), `search/cache.rs` (cache_media, remux, temps), `search/frontends.rs` (Invidious/Piped), `search/query.rs`.
- `remote.rs` (1571) → `remote/http.rs` (Request, read_request, write_*, percent_decode), `remote/routes.rs`, `remote/catalog.rs`, `remote/book.rs`, `remote/net.rs` (lan_urls, qr).
- `tags/mod.rs` (1138) → `fields.rs` (the 190-line `apply_fields` becomes a `(name, ItemKey)` table), `pictures.rs`, `dates.rs`.
- `player/mod.rs` (907) → keep `Player` façade; `player/transitions.rs` (`tick/on_eos/maybe_queue_gapless/on_gapless_started`), `player/load.rs` (`play_*`/`open_path*`), `player/settings.rs` (setters).
- `TagsView.tsx` (706) → `TagsView` shell, `FileListPane`, `DropZone` + `useWebviewDrop`, `useTagDoc` (load/save/batch/dirty), `LyricsCard`, `PicturesCard` + `CoverSearch`, `RawFramesCard`, `SaveBar`.
- `EqPanel.tsx` (631) → `EqPanel`, `useEqDraft`, `PresetGrid`, `ResponseCurve`, `BandEditor`, `MacroSliders`, `AutoEqImport`; `Toggle`/`GainSlider` → `ui/`.
- `DiscoverView.tsx` (568) → `discoverData.ts` (scan + cache + artist-image bus), `ArtistIndex`, `ArtistPage`, `AlbumPage`, `ArtistFace`.
- `PlayerView.tsx` (542) → `PlayerView` router, `PlaylistHeader`, `FolderHeader`, `MissingBanner`, `useTrackMenu`, `ui/SearchField`.
- `Playlists.tsx` (523) → `BrowseBack`, `LibraryNav`, `LibraryPage` (+`FolderArt`), `PlaylistsPage` (+`PlaylistCard`), `playlistActions.ts`. (Currently seven unrelated components live in this one file.)

### Hardcoded values that should be constants or config

Rust (→ `consts.rs` or next to the existing `SPEED_MIN/MAX`): tick 250 ms (`mod.rs:140`), viz 33 ms (`:152`), gapless preroll 1600 (`:825`), hop window 800 (`:823`), previous threshold 3000 (`:298`), folder cap 2000 (`:859`), publish 40 ms (`engine.rs:214`), default rate 48 000 in five places (`engine.rs:71,95,287`, `viz.rs:29`, `eq.rs:31`), Nyquist factors 0.45 vs 0.49 (`eq.rs:499, 586`), remote port 47321, `8` phones in three places (`remote.rs:647, 651, 897`), `MAX_CONNS` 24, 8 s/5 s/15 s timeouts, 1 500 000 art cap (`:1258`), 2 097 152 twice (`search.rs:116, 127`), thumb edges 720/640/80, cover edge 1600, thumb cache 256, batch 64, watcher 2 s/400 ms, MPRIS 1 s/30 s/400 ms/10 s, relink depth 5, speed range duplicated in `remote_page.html:746-747`.

TypeScript: `ROW = 58` (`TrackList.tsx:11`), `220px` nav, `280px` file pane, `92px` bar, `80ms` EQ debounce, `400ms` drop lock, `160/240px` rootMargins, caps `80/48/24/12`, `LOAD_LIMIT 2`, `ARTIST_CAP 15`, context-menu `220/32`, 10+ arbitrary font sizes (`text-[13px]`, `text-[15px]`, `text-[28px]`…) with no type scale in `tailwind.config.js`, `shadow-[0_18px_40px_rgb(0_0_0_/_0.28)]` inline ×3.

### Architecture smells

- `PlayerEngine` trait exists but `Player` holds `Arc<RodioEngine>` concretely (`mod.rs:101`) and requires a real `AppHandle` — the trait buys nothing and blocks testing.
- `AppError` is stringly typed: `Message(String)` everywhere; `Io/Json/Tag` variants leak raw messages ("io: No such file…") to the UI; nothing a caller can match on (`ToolMissing`, `Cancelled`, `NoDevice`).
- Hand-rolled global state beside Zustand: `browsePast`/`browseGeneration` (`browse.ts:71-72`), `discoverTracks` (`DiscoverView.tsx:23`), `pickedArtists` (`catalog.ts:155`), bespoke pub/sub `artistImageListeners` (`DiscoverView.tsx:322-340`). Untestable and StrictMode-hostile; the back stack is invisible to the store.
- Prop drilling `onMenu/onPlay/onContext` through `PlayerView → DiscoverView → ArtistPage/AlbumPage → TrackList` because `trackMenu()` lives in `PlayerView`.
- Dead code: `apply_tone_controls` (compiler warning, `eq.rs:490`), `build_tree` + `PlayerSnapshot.tree/queue` always empty (`scan.rs:211`, `mod.rs:206, 227`), `VizTap.rate` stored but ignored (`viz.rs:74` — bars map to different frequencies at 44.1 k vs 96 k), `positions`/`forget_position` for a removed feature, `MediaHit.url == page_url` always, `let hwnd = None` (`media.rs:32`), `library::prune_missing` doesn't prune (its own comment says so).
- Overly clever: `header + overrun + overrun` (`mod.rs:726`); the `queued <= 1 && position < 800` gapless-hop heuristic instead of a completion signal from `EqSource`.
- Doc divergences: config path; "Do not send picture bytes through IPC" vs `add_picture(Vec<u8>)`; "catch_unwind turns a Symphonia panic into an error" (only at open, not during decode); "Every request must carry the pairing code" (true, but S4 panics first).

### Test coverage summary

Tested (well): `queue.rs` 10, `eq.rs` 16, `playlists.rs` 12, `search.rs` 21 (JSON/paths only), `catalog.ts` 4, `eq.ts` 13, `theme.ts` 10, `format.ts` 7. All pure functions.

Zero tests: `engine.rs`, `watch.rs`, `lib.rs`, `error.rs`, `commands/mod.rs`, `read_request`/`percent_decode`/routing/SSE in `remote.rs`, corrupt-file load in `persist.rs`, `co64`/fragmented/fuzz in `mp4_recover.rs`, and **every React component and hook** (`vite.config.ts:31` is `environment: "node"`; no jsdom, no Testing Library). One tag test hardcodes `/home/napkin/Music/...` and silently passes elsewhere (`tags/mod.rs:1016-1021`). `library.rs:87` assertion is tautological. `remote.rs:1541` "no network" test passes only because the SVG namespace is `http://`.

Tooling: no ESLint, no Prettier, no `typecheck` script, no CI, `cargo fmt --check` mentioned in docs but not enforced.

---

## 4. Recommendation checklist

Ordered by (user harm × likelihood) ÷ effort. Each item names the file(s) it touches.

### Tier 0 — Stop the bleeding (do before anything else)

1. **`persist.rs`** — On parse failure rename the file to `state.json.corrupt-<ts>` and emit a status event instead of `unwrap_or_default()`; log/return `save_to` errors; add `#[serde(default)]` to the seven v1 fields. (S1)
2. **`player/engine.rs`** — `recv_timeout` in `RodioEngine::send`; a heartbeat (position not advancing while `playing` for N s) that rebuilds `OutputStream` + `Sink`; retry stream creation on demand when `try_default()` failed at startup. (P1)
3. **`src/App.tsx`, new `src/ui/ErrorBoundary.tsx`** — Root boundary that keeps the titlebar/close button alive, plus one per tab panel. (F1)
4. **`src/ui/ConfirmDialog.tsx`; `Playlists.tsx`, `PlayerView.tsx`, `TagsView.tsx`, `EqPanel.tsx`, `SettingsView.tsx`** — Confirm on delete playlist, "Apply to N", remove picture/field/preset/theme; wrap every destructive `await api.*` in try/catch → status. For batch tag write, show *which* fields will be written and skip blank fields unless explicitly cleared. (F2, S8)
5. **`remote.rs:1319`** — Rewrite `percent_decode` on `&[u8]` (`bytes[index+1..index+3]` + `from_utf8`); add parser tests. (S4)
6. **`player/convert.rs`** — Write transcodes to `.part` then rename; prune the cache by size (LRU, e.g. 500 MB cap) at startup and after each transcode; sweep `.audios-tmp.*` at startup. (U9)

### Tier 1 — Make playback trustworthy

7. **`player/mod.rs`** — Serialize `Player` ops behind one outer mutex or a command channel (the engine already has this pattern); commit queue state only *after* `set_uri` succeeds. (P2)
8. **`player/mod.rs`, `eq.rs` (`EqSource`)** — Replace the `queued <= 1 && position < 800` heuristic with an explicit completion counter from `EqSource::next`; on any queue/mode mutation while `pending_gapless`, clear and re-append. Collapse the duplicate `on_eos` branches. (P3)
9. **`player/engine.rs`** — Update `duration_ms`/`sample_rate` when an appended source *starts*, not only on `replace`. (P4)
10. **`player/mod.rs`** — On load failure in `on_eos`, mark the track failed and advance once; stop after a full failed cycle; never call `forget_current`/`persist.update` in the retry path. (P5)
11. **`player/mod.rs`** — `play()` on an empty sink with a current track should reload it; `previous()` must not trust a stale end-position. Handle `stop()`→`play()`. (P6)
12. **`player/mod.rs`** — Poison-tolerant lock helper (`lock().unwrap_or_else(|e| e.into_inner())`) like `EqShared`; `catch_unwind` around the ticker and viz loop bodies. (P8)
13. **`commands/mod.rs`** — Make `batch_write`, `scan_tracks`, `playlist_cover`, `write_tags`, `cover_art`, `remote_stop`, `play_playlist` async / `run_blocking`. (S3)
14. **`commands/mod.rs`, `search.rs`** — Generation token on `play_media`; kill the child on supersede; don't delete the cache file of an in-flight download. Add wall-clock timeouts to `.output()`. (S2, S7)
15. **`persist.rs`, `player/mod.rs`** — Debounce `Store::update` writes (coalesce within ~500 ms); remove the dead `positions`/`forget_position` path. (U10)
16. **`player/engine.rs`** — Track position in *file* time by integrating speed changes (or ask rodio for source position) instead of `wall × current_speed`. (P7)

### Tier 2 — Make it feel like a Linux app

17. **`lib.rs`, new `args.rs`; add `tauri-plugin-single-instance`** — Read `argv` at launch and on second-instance; `open_path` the file/folder. Only then keep `MimeType=` in `desktop.rs`. Escape `%`/`\`/`$` in `Exec`; write the icon once; call `update-desktop-database` if present; document uninstall. (U2)
18. **`styles.css`, `Sidebar.tsx`, `TagsView.tsx`, `NowPlayingFull.tsx`** — Collapse `.player-nav` to icons below ~720 px; make the Tags file pane a toggleable drawer; give `np-overlay` `overflow-auto`; replace fixed `minmax(240px)` with `minmax(min(240px,100%),1fr)`. Test at 480×320. (U1)
19. **`App.tsx`** — Always show `NowPlayingBar` (disabled transport, live volume/EQ/speed) so a fresh install has controls. (U3)
20. **`App.tsx`** — Scope shortcuts: skip when `target` is `BUTTON`/`SELECT`/`A` or inside `[role=dialog]`; require no modifier; add a `?` shortcut sheet. (F7)
21. **`Playlists.tsx`, `PlayerView.tsx`** — Add a visible `…` button on playlist/folder cards and the playlist header, reusing the `TrackList` pattern. (U4)
22. **`ContextMenu.tsx`** — `role="menu"`/`menuitem`, arrow/Home/End/Escape navigation, focus first item on open, keyboard submenu open, measure real height. (U5)
23. **`SeekBar.tsx`, `useAppStore.ts`** — Send `seek` on release (or throttle to ~100 ms), set `positionMs` optimistically, reset `useSmoothPosition` origin. (F4)
24. **`useAppStore.ts`** — Separate `error` from `status`; give info status a timeout and errors a dismiss; `role="status" aria-live="polite"` on the line. Path-check after `await` in `refreshCover`. (F6, F3)
25. **`NowPlayingBar.tsx`, `NowPlayingFull.tsx`, `TrackList.tsx`, `PlayerView.tsx`** — One `useTransport()` hook with in-flight guard and uniform `.catch → status`; stop double-applying snapshots. (F5)
26. **`App.tsx`** — Drop the `focus` listener (keep `visibilitychange`), debounce `refreshFromDisk`, decouple `FolderArt` from `libraryEpoch`. (F9)
27. **`TagsView.tsx`, `PlayerView.tsx`** — Real file drop via Tauri's `onDragDropEvent` on the whole window; remove the dead `File.path` branch. (U7)
28. **A11y sweep** — `aria-label` on every icon-only button; global `:focus-visible` ring; focus trap + initial focus in `ThemeBuilder` and `NowPlayingFull`; `role="list"` on `VirtualList`. (U6)
29. **`SearchView.tsx`, `TrackList.tsx`, `RemotePanel.tsx`, `remote_page.html`** — Loading skeletons; phone page shows "Remote restarted — rescan the QR" on 404 and surfaces `send()` failures. (U8)
30. **`watch.rs`, `relink.rs`** — Stop re-walking on watch failure (record failed dirs, back off); surface `max_user_watches` to the user; bound `relink` search to library roots (not their parents); move `audio_paths` out of `store.update`. (U12, U13)
31. **`remote.rs`** — Bind to the private interfaces from `lan_urls` (or make it a setting); per-IP failed-code lockout; set `stop`/emit status when `accept_loop` exits; per-request deadline. (S5, S6)

### Tier 3 — Structure and tooling (makes everything above safe to do)

32. **Root** — Add ESLint (`typescript-eslint`, `react-hooks`, `jsx-a11y`) + Prettier + `npm run typecheck`; `cargo clippy -D warnings` + `cargo fmt --check`; a GitHub Actions workflow running `npm test`, `npm run build`, `cargo test`, lint. Pin `--pool=threads` in `vite.config.ts`.
33. **`player/`** — Make `Player` generic over `PlayerEngine` + a small `Events` trait (instead of `AppHandle`); add `FakeEngine` with scripted `position/is_empty/queued`; drive `tick()` deterministically. Then write tests for P2–P6.
34. **`vite.config.ts`, `package.json`** — jsdom + Testing Library; test `browse.ts`, `browseCache.ts`, `covers` LRU, `placeFlyout`, `applySnapshot`, `SeekBar` drag, `VirtualList` clamping.
35. **New `src-tauri/src/paths.rs`** — Single `config_dir()/cache_dir()/data_dir()`; move covers/artist images to `data_dir`; fix the doc path.
36. **New `src-tauri/src/consts.rs`, `src/lib/constants.ts`, `tailwind.config.js`** — Hoist every magic number in §3; define a type scale.
37. **Dedupe** — `ffmpeg.rs` (one remux helper), one JPEG encoder, one tool resolver with a `OnceLock` cache for `find_ytdlp`/`node_available`, `src/ui/{Toggle,SearchField,ColorField,LazyImage,TransportRow}.tsx`, `playlistActions.ts`, `useTrackMenu()`.
38. **File splits** — As listed in §3, in this order: `Playlists.tsx` (cheapest, most misplaced), `player/mod.rs`, `TagsView.tsx`, `eq.rs`, `remote.rs`, `search.rs`, `EqPanel.tsx`, `DiscoverView.tsx`, `PlayerView.tsx`, `tags/mod.rs`.
39. **`error.rs`** — Typed variants (`NoDevice`, `ToolMissing{tool}`, `Cancelled`, `Missing{path}`, `Decode`) with a user-facing `Display`; map to distinct UI treatment.
40. **`useAppStore.ts`** — Move `browsePast`, `discoverTracks`, `pickedArtists`, `artistImageListeners` into the store or a `discover` slice.
41. **Docs** — Fix the four divergences; add a "Threat model" section for the remote; add a CHANGELOG.
42. **Cleanup** — Delete `apply_tone_controls`, `build_tree`, `PlayerSnapshot.tree/queue`, `MediaHit.page_url`, `hwnd`, `-webkit-app-region`; fix `VizTap.rate` so bands map correctly at 96 kHz; unify the volume clamp (1.0 / 1.5 / 3.0).

---

*Generated 2026-09-30. Line numbers refer to commit `75e68ac`.*
