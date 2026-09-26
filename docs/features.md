<p align="center">
  <img src="https://github.com/nnapkin12/AudiosPlayer/blob/main/public/audios.png" alt="Audios!" width="360">
</p>

# Audios! features

Scanned audio is mp3, flac, ogg, opus, m4a, mp4, aac, wav, aiff, wma, wavpack, and ape.

## Library

- Open a file, a folder, or a nested album tree. The folder list is the library, and it is kept across restarts.
- Library folders stay tied to the disk. Songs added or removed outside Audios! show up while the app is open and after a restart.
- A moved or renamed song or album is relinked when the file name is unique. Otherwise the item stays listed and **Locate** asks for the new path.
- The open library or playlist has its own search bar. It filters that list. The song list only draws the rows on screen.
- A track row shows a play button on the cover while the pointer is over it. The song that is playing shows pause, and clicking that pauses.

## Playlists

- Press + to create one. Add a file or a folder.
- A folder stays linked, so songs added or removed on disk show up in that playlist.
- Set a picture from a local image. With no picture, Audios! builds a 2×2 mosaic from up to four embedded covers.
- An open playlist, artist, or album shows that cover as a small square above the name, so more songs stay on screen.

## Discover

- Discover shows a random handful of artists already in the library.
- The search above that grid finds any of those artists and opens every song you have from them.

## Playback

- Queue with next and previous. Repeat is off, one song, or the whole queue. Shuffle is separate.
- Gapless playback is a switch under Settings → Playback. The next song is ready before the current one ends.
- ReplayGain, on the same page, uses track or album tags when they exist.
- Speed runs from 0.5× to 2×. Pitch moves with the tempo. The speed button sits beside the transport so play stays centered. Open it to step by 0.05 or type a number. The chosen speed is remembered.
- Leaving a song forgets its place. The next play starts at 0:00.
- The now-playing bar sits at the bottom. `f` opens the full view, `Esc` closes it. The full view takes its colors from the album art. The rest of the app stays on the saved theme.
- Keys, when a text field is not focused: Space play/pause, arrows seek ±5 seconds, `n` / `p` next / previous.
- On Linux, desktops that speak MPRIS (GNOME, KDE, and other bars) show the current song and art, and can play, pause, seek, and skip.

## Equalizer

Settings → Equalizer. One parametric curve for everything Audios! plays.

- A response graph shows that curve. Enable turns it on. Flat and Off leave the signal alone.
- Simple view: Bass, Low, Mid, Presence, and Air, each ±12 dB, plus Level and Prevent clipping.
- Advanced edits frequency, Q, and filter type (peak, low shelf, high shelf) on all ten bands.
- Paste an AutoEQ ParametricEQ block (preamp, PK, LSC, HSC).
- Built-in presets cannot be overwritten. Name and save up to 20 of your own. They live in `state.json` with custom themes.
- An older ten-band graphic curve loads as peaks at the old centers.

## Web remote

Settings → Remote, labeled Audios! web. **Start web remote** listens on the home network. Music keeps playing on the computer.

- The panel shows the link, a pairing code, a QR code, and any other LAN addresses. Copy puts the link on the clipboard.
- While the library is being read, the panel says so. After that it shows how many songs were indexed.
- The phone page has the cover, play and pause, seek, shuffle, volume, and a search of songs already in the library. A gear holds repeat and playback speed. That speed is the same control as the desktop.
- Phones that connect are listed as connected or last seen.
- **Stop web remote** closes the page. If a phone cannot open the link, allow the port in the firewall.

## Search

Text search lists YouTube, then SoundCloud. Paste a link to play a page from another site yt-dlp supports. Each result shows a thumbnail and the page URL, and the URL can be opened in the browser.

- Play downloads a temp file, remuxes it so the player can decode it, then plays it. That temp file is deleted when a different search track starts.
- Download keeps a copy through the system save dialog.
- A query with both artist and title usually ranks better than a title alone.
- Needs a current yt-dlp, ffmpeg, and curl. Distro packages of yt-dlp are often too old for current YouTube.

Search does not log into Spotify and cannot pull Spotify-hosted audio. It always writes a local file. It does not stream in the browser.

## Tags

The editor reads and writes the fields [lofty](https://docs.rs/lofty) understands for that container, including MP4 and M4A.

- Common fields, extra fields, lyrics, ReplayGain, MusicBrainz IDs, and custom frames.
- Artwork can be added, removed, or exported. **Find artwork** lists YouTube thumbnails for the track name and can embed one without leaving the editor. It does not download the audio.
- Batch apply writes the chosen fields across a folder.
- A save copies the file to a temp path, writes there, then replaces the original. The open list and the song that is playing update immediately.
- Save stays pinned at the bottom of the editor. Ctrl+S saves the open file.

## Appearance

Settings → Appearance.

- Themes: Dusk, Midnight, Slate, Paper.
- Accents: Blue, Amber, Sage, Rose, Violet.
- Minimize movement turns down UI animation. The choice is stored in `state.json`.
- The theme builder starts with grouped colors. Advanced edits each token. Name it and save. Custom themes live in `state.json`.

## Window

- The window resizes from its edges. The minimum size is 480×320, small enough for a tiling window manager.
- The title bar is frameless. Minimize, maximize, and close sit on the top right. A hairline sits on the outer edge.
- An AppImage writes its own menu entry on launch (`~/.local/share/applications`), under Multimedia. A `.deb` install does that through the package.

## Not included

- A Spotify client or downloader.
- In-browser streaming. Search always writes a local file.
