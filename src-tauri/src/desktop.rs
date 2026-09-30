//! User-level desktop entry for AppImage launches.
//!
//! A `.deb` install already places a desktop file in `/usr/share/applications`.
//! An AppImage does not. On first launch, and again if the AppImage moves,
//! write the XDG entry every desktop reads: `~/.local/share/applications`.
//!
//! Uninstall an AppImage copy:
//! `rm ~/.local/share/applications/com.audios.desktop.desktop
//!    ~/.local/share/icons/hicolor/128x128/apps/com.audios.desktop.png`
//! then `update-desktop-database ~/.local/share/applications` if that tool exists.

use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

pub fn install() {
    let Ok(appimage) = std::env::var("APPIMAGE") else {
        return;
    };
    if appimage.is_empty() || appimage.contains('\n') {
        return;
    }
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let home = PathBuf::from(home);
    let apps = home.join(".local/share/applications");
    let icons = home.join(".local/share/icons/hicolor/128x128/apps");
    if std::fs::create_dir_all(&apps).is_err() || std::fs::create_dir_all(&icons).is_err() {
        return;
    }
    let icon_path = icons.join("com.audios.desktop.png");
    write_icon_once(&icon_path);
    let desktop_path = apps.join("com.audios.desktop.desktop");
    let body = desktop_entry(&appimage, &icon_path.to_string_lossy());
    if std::fs::read_to_string(&desktop_path).ok().as_deref() == Some(body.as_str()) {
        return;
    }
    if std::fs::write(&desktop_path, body).is_ok() {
        refresh_desktop_db(&apps);
    }
}

fn write_icon_once(path: &Path) {
    if path.is_file() {
        if let Ok(existing) = std::fs::read(path) {
            if existing == ICON_PNG {
                return;
            }
        }
    }
    let _ = std::fs::write(path, ICON_PNG);
}

fn refresh_desktop_db(apps: &Path) {
    let _ = Command::new("update-desktop-database").arg(apps).status();
}

fn desktop_entry(exec: &str, icon: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name=Audios!\n\
         GenericName=Music Player\n\
         Comment=Play local music\n\
         Exec={} %U\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;Music;Player;\n\
         Keywords=Music;Audio;Player;\n\
         StartupWMClass=com.audios.desktop\n\
         MimeType=audio/flac;audio/mpeg;audio/mp4;audio/ogg;audio/opus;audio/x-flac;audio/x-vorbis+ogg;audio/wav;audio/x-wav;audio/aac;audio/x-m4a;audio/aiff;audio/x-aiff;\n\
         StartupNotify=true\n",
        quote_exec(exec)
    )
}

/// Desktop Entry Spec: quote the binary and escape `\`, `"`, `$`, `` ` ``, and `%`.
fn quote_exec(path: &str) -> String {
    let mut out = String::from("\"");
    for ch in path.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            '`' => out.push_str("\\`"),
            '%' => out.push_str("%%"),
            '\n' | '\r' => {}
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{desktop_entry, quote_exec};

    #[test]
    fn appimage_entry_lands_in_multimedia() {
        let body = desktop_entry("/home/listener/Audios!.AppImage", "/tmp/audios.png");
        assert!(body.contains("Categories=AudioVideo;Audio;Music;Player;"));
        assert!(body.contains("Exec=\"/home/listener/Audios!.AppImage\" %U"));
        assert!(body.contains("Name=Audios!"));
    }

    #[test]
    fn exec_escapes_reserved_characters() {
        assert_eq!(
            quote_exec("/home/a/$music/Audios 50%.AppImage"),
            "\"/home/a/\\$music/Audios 50%%.AppImage\""
        );
        assert_eq!(quote_exec(r#"C:\weird"#), r#""C:\\weird""#);
    }
}
