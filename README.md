<p align="center">
  <img src="public/audios.png" alt="Audios!" width="360">
</p>

<h1 align="center">Audios!</h1>

<p align="center">
  A Linux music player, tag editor, and library browser.<br>
  Local files, yt-dlp search, a parametric EQ, and remote on the same Wi-Fi.
</p>

<p align="center">
  <a href="docs/features.md">Features</a> ·
  <a href="docs/architecture.md">Architecture</a> ·
  <a href="CONTRIBUTING.md">Contributing</a> ·
  <a href="https://github.com/nnapkin12/AudiosPlayer/wiki">Wiki</a> ·
  <a href="LICENSE">MIT</a>
</p>

<p align="center">
  GitHub: <a href="https://github.com/nnapkin12/AudiosPlayer">AudiosPlayer</a>
</p>

## What it is

Audios! is a music player, plays music already on disk, and can pull extra tracks with [yt-dlp](https://github.com/yt-dlp/yt-dlp). Open a file or a folder tree and that becomes the library. Search lists YouTube and SoundCloud, then plays a temp file or saves a copy. The Tags tab writes metadata and artwork. Settings holds themes, the equalizer, and the web remote.

The UI is React. Playback, tags, search, and the remote run in Rust, hosted by [Tauri](https://tauri.app/).

It is not a Spotify client. Search cannot pull Spotify-hosted audio. Every search play writes a local file. There is no in-browser stream.

## Features

- **Library** — Nested folders, playlists, and a queue. Repeat, shuffle, gapless playback, ReplayGain, and speed from 0.5× to 2×. Folders stay linked to disk, so files added or removed outside the app show up. A moved file is relinked when the name is unique. Otherwise **Locate** asks for the new path.
- **Playlists and Discover** — Playlists are named lists. Set a picture, or leave the 2×2 mosaic built from track art. Adding a folder keeps that folder link. Library and playlist views each have a search bar. Discover shows artists already in the library.
- **Equalizer** — One parametric curve for everything Audios! plays (Settings → Equalizer). Tone sliders, a ten-band editor, a response graph, and AutoEQ paste. Saved profiles live with the rest of app state.
- **Search** — A song and artist query, or a pasted link. Text search covers YouTube and SoundCloud. A link can be any site yt-dlp supports. Play uses a temp file, deleted when another search track starts. Download keeps a copy through the system save dialog.
- **Tags** — Title, artists, album, lyrics, ReplayGain, MusicBrainz IDs, custom fields, and artwork, including Find artwork. Batch apply across a folder. A save updates the open list and the song that is playing.
- **Remote** — Settings → Remote (Audios! web) listens on the home network. The link and QR open a phone page: cover, play and pause, seek, shuffle, volume, and a search of songs already in the library. A pairing code is part of the URL. Sound stays on the computer.
- **Desktop** — Linux MPRIS shows the current song and art, and accepts play, pause, seek, next, and previous. Themes are Dusk, Midnight, Slate, and Paper, plus a theme builder.

A longer list is in [docs/features.md](docs/features.md).

## Releases

GitHub Release assets are a `.deb` and an AppImage.

Search is not bundled. It needs a current [yt-dlp](https://github.com/yt-dlp/yt-dlp), ffmpeg, and curl on the machine. Distro copies of yt-dlp are often too old for current YouTube.

## License

[MIT](LICENSE). Logo marks: [CREDITS.md](CREDITS.md).
