//! Files and folders passed on the command line, including a second instance.

use std::path::{Path, PathBuf};

use crate::player::Player;

/// Paths from `argv` that exist on disk. Skips the program name, flags, and
/// URLs that are not `file://`. Relative names resolve against `cwd`.
pub fn media_paths(argv: impl IntoIterator<Item = impl AsRef<str>>, cwd: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut skip_value = false;
    for (index, arg) in argv.into_iter().enumerate() {
        let arg = arg.as_ref().trim();
        if index == 0 || arg.is_empty() || arg == "--" {
            continue;
        }
        if skip_value {
            skip_value = false;
            continue;
        }
        if arg.starts_with('-') {
            if takes_value(arg) {
                skip_value = true;
            }
            continue;
        }
        if let Some(path) = path_from_arg(arg, cwd) {
            out.push(path);
        }
    }
    out
}

/// Play one path, or queue several. Runs off the GTK thread because a folder
/// scan can take a while.
pub fn open_in_player(player: &Player, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let player = player.clone();
    let _ = std::thread::Builder::new()
        .name("audios-open".into())
        .spawn(move || {
            let strings: Vec<String> = paths
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            let result = if strings.len() == 1 {
                player.open_path(&strings[0])
            } else {
                player.play_queue_paths(strings, None)
            };
            if let Err(error) = result {
                eprintln!("Audios! could not open launched files: {error}");
            }
        });
}

fn takes_value(flag: &str) -> bool {
    matches!(
        flag,
        "--class" | "--name" | "--display" | "--gtk-module" | "-e"
    )
}

fn path_from_arg(arg: &str, cwd: &Path) -> Option<PathBuf> {
    let decoded = if let Some(rest) = arg.strip_prefix("file://") {
        decode_file_uri(rest)?
    } else if arg.contains("://") {
        return None;
    } else {
        arg.to_string()
    };
    let path = PathBuf::from(&decoded);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    path.exists().then_some(path)
}

fn decode_file_uri(rest: &str) -> Option<String> {
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    percent_decode(rest)
}

fn percent_decode(input: &str) -> Option<String> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hi = from_hex(bytes[index + 1])?;
            let lo = from_hex(bytes[index + 2])?;
            out.push((hi << 4) | lo);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn skips_flags_and_the_program_name() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("a.mp3");
        fs::write(&song, []).unwrap();
        let argv = vec![
            "/usr/bin/audios".into(),
            "--class".into(),
            "com.audios.desktop".into(),
            song.to_string_lossy().into_owned(),
        ];
        assert_eq!(media_paths(argv, dir.path()), vec![song]);
    }

    #[test]
    fn resolves_relative_names() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("track.flac"), []).unwrap();
        let argv = ["audios", "track.flac"];
        assert_eq!(
            media_paths(argv, dir.path()),
            vec![dir.path().join("track.flac")]
        );
    }

    #[test]
    fn accepts_file_uris() {
        let dir = tempfile::tempdir().unwrap();
        let song = dir.path().join("song name.mp3");
        fs::write(&song, []).unwrap();
        let uri = format!("file://{}", song.to_string_lossy().replace(' ', "%20"));
        assert_eq!(media_paths(["audios", &uri], Path::new("/")), vec![song]);
    }

    #[test]
    fn ignores_http_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let argv = [
            "audios",
            "https://example/a.mp3",
            "/no/such/file.mp3",
            dir.path().to_str().unwrap(),
        ];
        assert_eq!(
            media_paths(argv, dir.path()),
            vec![dir.path().to_path_buf()]
        );
    }
}
