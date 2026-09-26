//! LAN remote. The phone is a controller; sound stays on this computer.
//!
//! Started from Settings. A pairing code is required on every request.
//! The page is a static file, not the desktop React app. Library search
//! uses an index that exists only while the remote is running. Commands
//! play by index id, never by a path from the phone.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};
use crate::persist::Store;
use crate::player::queue::RepeatMode;
use crate::player::scan::{audio_paths, track_from_path, Track};
use crate::player::Player;

const PAGE: &str = include_str!("remote_page.html");
const PREFERRED_PORT: u16 = 47321;
const MAX_CONNS: usize = 24;
const SEARCH_LIMIT: usize = 24;
const HEADER_LIMIT: usize = 8 * 1024;
const BODY_LIMIT: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteClient {
    pub id: u64,
    pub name: String,
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteStatus {
    pub running: bool,
    pub url: Option<String>,
    pub urls: Vec<String>,
    pub code: Option<String>,
    pub qr_svg: Option<String>,
    pub indexing: bool,
    pub songs: u32,
    pub clients: Vec<RemoteClient>,
}

impl RemoteStatus {
    fn idle() -> Self {
        Self {
            running: false,
            url: None,
            urls: Vec::new(),
            code: None,
            qr_svg: None,
            indexing: false,
            songs: 0,
            clients: Vec::new(),
        }
    }
}

pub struct Remote {
    app: AppHandle,
    player: Player,
    store: Store,
    inner: Mutex<Option<Running>>,
}

struct Running {
    port: u16,
    shared: Arc<Shared>,
    joins: Vec<JoinHandle<()>>,
}

struct Shared {
    player: Player,
    store: Store,
    code: String,
    url: String,
    urls: Vec<String>,
    qr_svg: String,
    stop: Arc<AtomicBool>,
    inflight: AtomicUsize,
    catalog: Arc<Catalog>,
    art: Arc<Art>,
    live: Arc<Live>,
    book: Arc<Book>,
}

struct Catalog {
    ready: AtomicBool,
    entries: Mutex<Vec<Entry>>,
}

struct Entry {
    title: String,
    artist: String,
    path: String,
    hay: String,
}

struct Art {
    mu: Mutex<ArtInner>,
}

struct ArtInner {
    path: String,
    jpeg: Vec<u8>,
    mime: String,
    rev: u64,
}

struct Live {
    mu: Mutex<LiveInner>,
    cv: Condvar,
}

struct LiveInner {
    json: String,
    version: u64,
}

struct Book {
    clients: Mutex<Vec<SeatInfo>>,
    connected: AtomicUsize,
    next_id: AtomicUsize,
    wake_mu: Mutex<()>,
    wake: Condvar,
    emit: Mutex<Option<Emit>>,
}

struct SeatInfo {
    id: u64,
    ip: String,
    name: String,
    sockets: u32,
}

struct Emit {
    app: AppHandle,
    shared: Weak<Shared>,
}

impl Clone for Emit {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            shared: self.shared.clone(),
        }
    }
}

struct Seat {
    book: Arc<Book>,
    id: u64,
}

impl Drop for Seat {
    fn drop(&mut self) {
        {
            let mut clients = self.book.clients.lock().expect("clients");
            if let Some(client) = clients.iter_mut().find(|client| client.id == self.id) {
                client.sockets = client.sockets.saturating_sub(1);
            }
        }
        self.book.connected.fetch_sub(1, Ordering::Relaxed);
        self.book.notify_wake();
        self.book.poke();
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WireNow {
    title: String,
    artist: String,
    album: String,
    playing: bool,
    position_ms: u64,
    duration_ms: u64,
    volume: f64,
    muted: bool,
    repeat: RepeatMode,
    shuffle: bool,
    speed: f64,
    art_rev: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Control {
    op: String,
    position_ms: Option<u64>,
    volume: Option<f64>,
    shuffle: Option<bool>,
    repeat: Option<RepeatMode>,
    speed: Option<f64>,
    id: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Hit {
    id: u32,
    title: String,
    artist: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchBody {
    indexing: bool,
    hits: Vec<Hit>,
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Remote {
    pub fn new(app: AppHandle, player: Player, store: Store) -> Self {
        Self {
            app,
            player,
            store,
            inner: Mutex::new(None),
        }
    }

    pub fn status(&self) -> RemoteStatus {
        let guard = self.inner.lock().expect("remote");
        match guard.as_ref() {
            Some(running) => running.shared.status(),
            None => RemoteStatus::idle(),
        }
    }

    pub fn start(&self) -> AppResult<RemoteStatus> {
        {
            let guard = self.inner.lock().expect("remote");
            if let Some(running) = guard.as_ref() {
                return Ok(running.shared.status());
            }
        }

        let listener = bind_listener()?;
        let port = listener
            .local_addr()
            .map_err(|_| AppError::msg("Could not open a port for the web remote"))?
            .port();
        let code = pairing_code();
        let urls = lan_urls(port, &code);
        let url = urls.first().cloned().unwrap_or_default();
        let qr_svg = if url.is_empty() {
            String::new()
        } else {
            qr_svg(&url).unwrap_or_default()
        };

        let book = Arc::new(Book {
            clients: Mutex::new(Vec::new()),
            connected: AtomicUsize::new(0),
            next_id: AtomicUsize::new(1),
            wake_mu: Mutex::new(()),
            wake: Condvar::new(),
            emit: Mutex::new(None),
        });
        let shared = Arc::new(Shared {
            player: self.player.clone(),
            store: self.store.clone(),
            code,
            url,
            urls,
            qr_svg,
            stop: Arc::new(AtomicBool::new(false)),
            inflight: AtomicUsize::new(0),
            catalog: Arc::new(Catalog {
                ready: AtomicBool::new(false),
                entries: Mutex::new(Vec::new()),
            }),
            art: Arc::new(Art {
                mu: Mutex::new(ArtInner {
                    path: String::new(),
                    jpeg: Vec::new(),
                    mime: String::new(),
                    rev: 0,
                }),
            }),
            live: Arc::new(Live {
                mu: Mutex::new(LiveInner {
                    json: String::new(),
                    version: 0,
                }),
                cv: Condvar::new(),
            }),
            book: Arc::clone(&book),
        });
        *book.emit.lock().expect("emit") = Some(Emit {
            app: self.app.clone(),
            shared: Arc::downgrade(&shared),
        });

        let mut joins = Vec::new();
        joins.push(spawn_named("audios-remote", {
            let shared = Arc::clone(&shared);
            move || accept_loop(listener, shared)
        }));
        joins.push(spawn_named("audios-remote-live", {
            let shared = Arc::clone(&shared);
            move || publish_loop(shared)
        }));
        joins.push(spawn_named("audios-remote-index", {
            let shared = Arc::clone(&shared);
            move || index_loop(shared)
        }));

        let running = Running {
            port,
            shared: Arc::clone(&shared),
            joins,
        };
        let status = running.shared.status();
        *self.inner.lock().expect("remote") = Some(running);
        let _ = self.app.emit("remote://status", &status);
        Ok(status)
    }

    pub fn stop(&self) -> RemoteStatus {
        let running = self.inner.lock().expect("remote").take();
        if let Some(running) = running {
            running.shutdown();
        }
        let status = RemoteStatus::idle();
        let _ = self.app.emit("remote://status", &status);
        status
    }
}

impl Running {
    fn shutdown(self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.book.notify_wake();
        self.shared.live.notify();
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        for join in self.joins {
            let _ = join.join();
        }
        self.shared.catalog.clear();
        self.shared.art.clear();
    }
}

impl Shared {
    fn status(&self) -> RemoteStatus {
        if self.stop.load(Ordering::Relaxed) {
            return RemoteStatus::idle();
        }
        let ready = self.catalog.ready.load(Ordering::Relaxed);
        let songs = self.catalog.len() as u32;
        let clients = self.book.listed();
        RemoteStatus {
            running: true,
            url: if self.url.is_empty() {
                None
            } else {
                Some(self.url.clone())
            },
            urls: self.urls.clone(),
            code: Some(self.code.clone()),
            qr_svg: if self.qr_svg.is_empty() {
                None
            } else {
                Some(self.qr_svg.clone())
            },
            indexing: !ready,
            songs: if ready { songs } else { 0 },
            clients,
        }
    }

    fn wire_now(&self) -> WireNow {
        let transport = self.player.transport();
        let art_rev = self.art.sync(&transport.path);
        let title = display_title(&transport.title, &transport.path);
        let artist = if transport.artist.is_empty() {
            transport.album_artist
        } else {
            transport.artist
        };
        WireNow {
            title,
            artist,
            album: transport.album,
            playing: transport.playing,
            position_ms: transport.position_ms,
            duration_ms: transport.duration_ms,
            volume: transport.volume,
            muted: transport.muted,
            repeat: transport.repeat,
            shuffle: transport.shuffle,
            speed: transport.speed,
            art_rev,
        }
    }
}

impl Catalog {
    fn len(&self) -> usize {
        self.entries.lock().expect("catalog").len()
    }

    fn clear(&self) {
        let mut entries = self.entries.lock().expect("catalog");
        entries.clear();
        entries.shrink_to_fit();
        self.ready.store(false, Ordering::Relaxed);
    }

    fn path(&self, id: u32) -> Option<String> {
        self.entries
            .lock()
            .expect("catalog")
            .get(id as usize)
            .map(|entry| entry.path.clone())
    }

    fn search(&self, query: &str) -> SearchBody {
        let indexing = !self.ready.load(Ordering::Relaxed);
        let needle = query.trim().to_lowercase();
        if needle.is_empty() || needle.chars().count() > 80 {
            return SearchBody {
                indexing,
                hits: Vec::new(),
            };
        }
        let entries = self.entries.lock().expect("catalog");
        let mut hits = Vec::new();
        for (id, entry) in entries.iter().enumerate() {
            if entry.hay.contains(&needle) {
                hits.push(Hit {
                    id: id as u32,
                    title: entry.title.clone(),
                    artist: entry.artist.clone(),
                });
                if hits.len() == SEARCH_LIMIT {
                    break;
                }
            }
        }
        SearchBody { indexing, hits }
    }
}

impl Art {
    fn sync(&self, path: &str) -> u64 {
        {
            let guard = self.mu.lock().expect("art");
            if guard.path == path {
                return if guard.jpeg.is_empty() { 0 } else { guard.rev };
            }
        }
        let loaded = if path.is_empty() {
            None
        } else {
            load_cover(path)
        };
        let mut guard = self.mu.lock().expect("art");
        if guard.path != path {
            guard.rev = guard.rev.wrapping_add(1).max(1);
            guard.path = path.to_string();
            match loaded {
                Some((mime, jpeg)) => {
                    guard.mime = mime;
                    guard.jpeg = jpeg;
                }
                None => {
                    guard.mime.clear();
                    guard.jpeg.clear();
                }
            }
        }
        if guard.jpeg.is_empty() {
            0
        } else {
            guard.rev
        }
    }

    fn bytes(&self) -> Option<(String, Vec<u8>)> {
        let guard = self.mu.lock().expect("art");
        if guard.jpeg.is_empty() {
            None
        } else {
            Some((guard.mime.clone(), guard.jpeg.clone()))
        }
    }

    fn clear(&self) {
        let mut guard = self.mu.lock().expect("art");
        if guard.path.is_empty() && guard.jpeg.is_empty() {
            return;
        }
        guard.path.clear();
        guard.jpeg.clear();
        guard.jpeg.shrink_to_fit();
        guard.mime.clear();
        guard.rev = 0;
    }
}

impl Live {
    fn publish(&self, json: String) {
        let mut guard = self.mu.lock().expect("live");
        if guard.json == json {
            return;
        }
        guard.json = json;
        guard.version = guard.version.wrapping_add(1);
        self.cv.notify_all();
    }

    fn notify(&self) {
        self.cv.notify_all();
    }

    fn current(&self) -> Option<(u64, String)> {
        let guard = self.mu.lock().expect("live");
        if guard.json.is_empty() {
            None
        } else {
            Some((guard.version, guard.json.clone()))
        }
    }

    fn wait(&self, seen: &mut u64, timeout: Duration) -> Option<String> {
        let guard = self.mu.lock().expect("live");
        let (guard, _) = self
            .cv
            .wait_timeout_while(guard, timeout, |inner| inner.version == *seen)
            .expect("live wait");
        if guard.version == *seen {
            None
        } else {
            *seen = guard.version;
            Some(guard.json.clone())
        }
    }
}

impl Book {
    fn notify_wake(&self) {
        let _guard = self.wake_mu.lock().expect("wake");
        self.wake.notify_all();
    }

    fn wait_wake(&self, timeout: Duration) {
        let guard = self.wake_mu.lock().expect("wake");
        let _ = self.wake.wait_timeout(guard, timeout);
    }

    /// Wait until a phone connects or the timeout fires. The check and the
    /// wait share the mutex so a connect cannot land in between.
    fn wait_until_connected(&self, timeout: Duration) {
        let guard = self.wake_mu.lock().expect("wake");
        if self.connected.load(Ordering::Relaxed) > 0 {
            return;
        }
        let _ = self.wake.wait_timeout(guard, timeout);
    }

    fn poke(&self) {
        let emit = self.emit.lock().expect("emit").clone();
        let Some(emit) = emit else { return };
        let Some(shared) = emit.shared.upgrade() else {
            return;
        };
        let status = shared.status();
        let _ = emit.app.emit("remote://status", &status);
    }

    fn listed(&self) -> Vec<RemoteClient> {
        let clients = self.clients.lock().expect("clients");
        let mut listed: Vec<RemoteClient> = clients
            .iter()
            .map(|client| RemoteClient {
                id: client.id,
                name: client.name.clone(),
                connected: client.sockets > 0,
            })
            .collect();
        listed.sort_by(|a, b| b.connected.cmp(&a.connected).then(a.name.cmp(&b.name)));
        listed
    }

    fn join(self: &Arc<Self>, addr: SocketAddr, ua: &str) -> Option<Seat> {
        let ip = addr.ip().to_string();
        let name = device_name(ua);
        let id = {
            let mut clients = self.clients.lock().expect("clients");
            if let Some(existing) = clients.iter_mut().find(|client| client.ip == ip) {
                existing.sockets = existing.sockets.saturating_add(1);
                existing.name = name;
                existing.id
            } else if clients.len() >= 8 && clients.iter().all(|client| client.sockets > 0) {
                return None;
            } else {
                if clients.len() >= 8 {
                    if let Some(pos) = clients.iter().position(|client| client.sockets == 0) {
                        clients.remove(pos);
                    }
                }
                let id = self.next_id.fetch_add(1, Ordering::Relaxed) as u64;
                clients.push(SeatInfo {
                    id,
                    ip,
                    name,
                    sockets: 1,
                });
                id
            }
        };
        self.connected.fetch_add(1, Ordering::Relaxed);
        self.notify_wake();
        self.poke();
        Some(Seat {
            book: Arc::clone(self),
            id,
        })
    }
}

fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(work)
        .expect(name)
}

fn bind_listener() -> AppResult<TcpListener> {
    TcpListener::bind(("0.0.0.0", PREFERRED_PORT))
        .or_else(|_| TcpListener::bind(("0.0.0.0", 0)))
        .map_err(|_| AppError::msg("Could not open a port for the web remote"))
}

fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        match listener.accept() {
            Ok((stream, addr)) => {
                if shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                let shared = Arc::clone(&shared);
                let _ = std::thread::Builder::new()
                    .name("audios-remote-client".into())
                    .spawn(move || handle_client(stream, addr, &shared));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

fn publish_loop(shared: Arc<Shared>) {
    let mut holding_art = false;
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        if shared.book.connected.load(Ordering::Relaxed) == 0 {
            if holding_art {
                shared.art.clear();
                holding_art = false;
            }
            shared.book.wait_until_connected(Duration::from_secs(2));
            continue;
        }
        holding_art = true;
        if let Ok(json) = serde_json::to_string(&shared.wire_now()) {
            shared.live.publish(json);
        }
        shared.book.wait_wake(Duration::from_millis(250));
    }
}

fn index_loop(shared: Arc<Shared>) {
    let mut seen = HashSet::new();
    let mut batch = Vec::new();
    let roots = crate::library::list(&shared.store);
    for root in &roots {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(files) = audio_paths(Path::new(root)) else {
            continue;
        };
        for file in files {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            push_entry(&mut batch, &mut seen, &file);
            if batch.len() >= 64 {
                flush(&shared.catalog, &mut batch);
            }
        }
    }
    for playlist in crate::playlists::list(&shared.store) {
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        for path in crate::playlists::flatten_paths(&playlist) {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            push_entry(&mut batch, &mut seen, Path::new(&path));
            if batch.len() >= 64 {
                flush(&shared.catalog, &mut batch);
            }
        }
    }
    flush(&shared.catalog, &mut batch);
    if shared.stop.load(Ordering::Relaxed) {
        return;
    }
    shared.catalog.ready.store(true, Ordering::Relaxed);
    shared.book.poke();
}

fn flush(catalog: &Catalog, batch: &mut Vec<Entry>) {
    if batch.is_empty() {
        return;
    }
    catalog.entries.lock().expect("catalog").append(batch);
}

fn push_entry(batch: &mut Vec<Entry>, seen: &mut HashSet<String>, path: &Path) {
    if !path.is_file() {
        return;
    }
    let key = path.to_string_lossy().into_owned();
    if !seen.insert(key) {
        return;
    }
    batch.push(entry_from(track_from_path(path)));
}

fn entry_from(track: Track) -> Entry {
    let file = Path::new(&track.path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = Path::new(&file)
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.clone());
    let artist = if track.artist.is_empty() {
        track.album_artist.clone()
    } else {
        track.artist.clone()
    };
    let hay = format!(
        "{} {} {} {} {} {}",
        track.title, track.artist, track.album_artist, track.album, file, stem
    )
    .to_lowercase();
    let title = if track.title.trim().is_empty() {
        stem
    } else {
        track.title
    };
    Entry {
        title,
        artist,
        path: track.path,
        hay,
    }
}

struct Inflight<'a>(&'a AtomicUsize);

impl Drop for Inflight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn handle_client(mut stream: TcpStream, addr: SocketAddr, shared: &Shared) {
    let _ = stream.set_nodelay(true);
    if shared.inflight.load(Ordering::Relaxed) >= MAX_CONNS {
        let _ = write_text(
            &mut stream,
            "503 Service Unavailable",
            "text/plain; charset=utf-8",
            "Too many connections.",
        );
        return;
    }
    shared.inflight.fetch_add(1, Ordering::Relaxed);
    let _slot = Inflight(&shared.inflight);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(8)));
    let Ok(request) = read_request(&mut stream) else {
        return;
    };
    if !query_code(&request).is_some_and(|code| same_code(code, &shared.code)) {
        let _ = write_text(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "Not found",
        );
        return;
    }
    let result = match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => write_text(&mut stream, "200 OK", "text/html; charset=utf-8", PAGE),
        ("GET", "/events") => serve_events(stream, addr, &request, shared),
        ("GET", "/art") => serve_art(&mut stream, shared),
        ("GET", "/search") => {
            let query = query_value(&request, "q").unwrap_or("");
            let body = shared.catalog.search(query);
            match serde_json::to_string(&body) {
                Ok(json) => write_text(&mut stream, "200 OK", "application/json", &json),
                Err(_) => write_text(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    "Could not search",
                ),
            }
        }
        ("POST", "/control") => serve_control(&mut stream, shared, &request.body),
        _ => write_text(
            &mut stream,
            "404 Not Found",
            "text/plain; charset=utf-8",
            "Not found",
        ),
    };
    let _ = result;
}

fn serve_events(
    mut stream: TcpStream,
    addr: SocketAddr,
    request: &Request,
    shared: &Shared,
) -> std::io::Result<()> {
    if shared.book.connected.load(Ordering::Relaxed) >= 8 {
        return write_text(
            &mut stream,
            "503 Service Unavailable",
            "text/plain; charset=utf-8",
            "Too many phones.",
        );
    }
    let ua = header(request, "user-agent").unwrap_or("");
    let Some(_seat) = shared.book.join(addr, ua) else {
        return write_text(
            &mut stream,
            "503 Service Unavailable",
            "text/plain; charset=utf-8",
            "Too many phones.",
        );
    };
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    write_raw(
        &mut stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n",
    )?;
    let mut seen = 0u64;
    if let Some((version, json)) = shared.live.current() {
        write_event(&mut stream, &json)?;
        seen = version;
    }
    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        match shared.live.wait(&mut seen, Duration::from_secs(15)) {
            Some(json) => write_event(&mut stream, &json)?,
            None => {
                stream.write_all(b": ping\n\n")?;
                stream.flush()?;
            }
        }
    }
    Ok(())
}

fn serve_art(stream: &mut TcpStream, shared: &Shared) -> std::io::Result<()> {
    match shared.art.bytes() {
        Some((mime, bytes)) => write_bytes(stream, "200 OK", &mime, &bytes),
        None => write_bytes(stream, "204 No Content", "text/plain", b""),
    }
}

fn serve_control(stream: &mut TcpStream, shared: &Shared, body: &[u8]) -> std::io::Result<()> {
    if body.len() > BODY_LIMIT {
        return write_text(
            stream,
            "413 Payload Too Large",
            "text/plain; charset=utf-8",
            "Too large",
        );
    }
    match apply_control(shared, body) {
        Ok(now) => match serde_json::to_string(&now) {
            Ok(json) => write_text(stream, "200 OK", "application/json", &json),
            Err(_) => write_text(
                stream,
                "500 Internal Server Error",
                "text/plain; charset=utf-8",
                "Could not answer",
            ),
        },
        Err(error) => write_text(
            stream,
            "400 Bad Request",
            "text/plain; charset=utf-8",
            &error.to_string(),
        ),
    }
}

fn apply_control(shared: &Shared, body: &[u8]) -> AppResult<WireNow> {
    let control: Control =
        serde_json::from_slice(body).map_err(|_| AppError::msg("Bad request"))?;
    match control.op.as_str() {
        "toggle" => {
            shared.player.toggle()?;
        }
        "next" => {
            shared.player.next()?;
        }
        "previous" => {
            shared.player.previous()?;
        }
        "seek" => {
            let position = control
                .position_ms
                .ok_or_else(|| AppError::msg("Missing position"))?;
            shared.player.seek(position)?;
        }
        "volume" => {
            let volume = control
                .volume
                .ok_or_else(|| AppError::msg("Missing volume"))?;
            if shared.player.transport().muted {
                shared.player.set_muted(false)?;
            }
            shared.player.set_volume(volume.clamp(0.0, 1.0))?;
        }
        "shuffle" => {
            let shuffle = control
                .shuffle
                .ok_or_else(|| AppError::msg("Missing shuffle"))?;
            shared.player.set_shuffle(shuffle)?;
        }
        "repeat" => {
            let repeat = control
                .repeat
                .ok_or_else(|| AppError::msg("Missing repeat"))?;
            shared.player.set_repeat(repeat)?;
        }
        "speed" => {
            let speed = control
                .speed
                .ok_or_else(|| AppError::msg("Missing speed"))?;
            shared.player.set_speed(speed)?;
        }
        "play" => {
            let id = control.id.ok_or_else(|| AppError::msg("Missing song"))?;
            let path = shared
                .catalog
                .path(id)
                .ok_or_else(|| AppError::msg("That song is not in the library"))?;
            shared.player.play_folder_file(&path)?;
        }
        _ => return Err(AppError::msg("Unknown control")),
    }
    Ok(shared.wire_now())
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    let header_end = loop {
        if buf.len() > HEADER_LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
        let read = stream.read(&mut tmp)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "closed",
            ));
        }
        buf.extend_from_slice(&tmp[..read]);
        if let Some(pos) = find_header_end(&buf) {
            break pos;
        }
    };
    let head = std::str::from_utf8(&buf[..header_end]).unwrap_or("");
    let mut lines = head.split("\r\n");
    let Some(start) = lines.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "no request",
        ));
    };
    let mut parts = start.split(' ');
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/");
    let (path, query_raw) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let mut body = buf[header_end + 4..].to_vec();
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    if length > BODY_LIMIT {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "body too large",
        ));
    }
    while body.len() < length {
        let read = stream.read(&mut tmp)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..read]);
        if body.len() > BODY_LIMIT {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "body too large",
            ));
        }
    }
    body.truncate(length);
    Ok(Request {
        method,
        path: path.to_string(),
        query: parse_query(query_raw),
        headers,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn query_value<'a>(request: &'a Request, key: &str) -> Option<&'a str> {
    request
        .query
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

fn query_code(request: &Request) -> Option<&str> {
    query_value(request, "code")
}

fn header<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn write_text(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    write_bytes(stream, status, content_type, body.as_bytes())
}

fn write_bytes(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn write_raw(stream: &mut TcpStream, header: &str) -> std::io::Result<()> {
    stream.write_all(header.as_bytes())?;
    stream.flush()
}

fn write_event(stream: &mut TcpStream, json: &str) -> std::io::Result<()> {
    stream.write_all(b"data: ")?;
    stream.write_all(json.as_bytes())?;
    stream.write_all(b"\n\n")?;
    stream.flush()
}

fn load_cover(path: &str) -> Option<(String, Vec<u8>)> {
    let cover = crate::tags::cover_for(path).ok().flatten()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(cover.data_base64)
        .ok()?;
    if bytes.is_empty() || bytes.len() > 1_500_000 {
        return None;
    }
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some((mime.to_string(), bytes))
}

fn display_title(title: &str, path: &str) -> String {
    let trimmed = title.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    if path.is_empty() {
        return String::new();
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name.to_string(),
    }
}

fn pairing_code() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    (0..6)
        .map(|_| {
            let index = fastrand::usize(..ALPHABET.len());
            ALPHABET[index] as char
        })
        .collect()
}

fn same_code(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

fn parse_query(raw: &str) -> Vec<(String, String)> {
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &raw[index + 1..index + 3];
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    out.push(value);
                    index += 3;
                } else {
                    out.push(b'%');
                    index += 1;
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn device_name(ua: &str) -> String {
    let browser = if ua.contains("Edg/") {
        "Edge"
    } else if ua.contains("OPR/") || ua.contains("Opera") {
        "Opera"
    } else if ua.contains("CriOS") || ua.contains("Chrome/") {
        "Chrome"
    } else if ua.contains("FxiOS") || ua.contains("Firefox/") {
        "Firefox"
    } else if ua.contains("Safari/") {
        "Safari"
    } else {
        "Browser"
    };
    let device = if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Mac OS") {
        "Mac"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        ""
    };
    if device.is_empty() {
        browser.to_string()
    } else {
        format!("{browser} on {device}")
    }
}

fn lan_urls(port: u16, code: &str) -> Vec<String> {
    let mut ips =
        fib_local_ipv4(&std::fs::read_to_string("/proc/net/fib_trie").unwrap_or_default());
    if let Some(routed) = route_ipv4() {
        ips.retain(|ip| ip != &routed);
        ips.insert(0, routed);
    }
    ips.into_iter()
        .map(|ip| format!("http://{ip}:{port}/?code={code}"))
        .collect()
}

fn route_ipv4() -> Option<String> {
    // UDP connect picks a source address and does not send a packet.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if is_private_v4(ip) => Some(ip.to_string()),
        _ => None,
    }
}

fn fib_local_ipv4(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut last: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("|-- ") {
            let ip = rest.split_whitespace().next().unwrap_or(rest);
            if ip.parse::<Ipv4Addr>().is_ok() {
                last = Some(ip);
            }
            continue;
        }
        if trimmed.contains("host LOCAL") {
            if let Some(ip) = last.take() {
                if let Ok(addr) = ip.parse::<Ipv4Addr>() {
                    if is_private_v4(addr) {
                        if !out.iter().any(|have| have == ip) {
                            out.push(ip.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

fn qr_svg(text: &str) -> Option<String> {
    use qrcode::QrCode;
    let code = QrCode::new(text.as_bytes()).ok()?;
    let quiet = 2i32;
    let width = code.width() as i32;
    let size = width + quiet * 2;
    let mut path = String::new();
    for y in 0..width {
        for x in 0..width {
            if code[(x as usize, y as usize)] == qrcode::Color::Dark {
                let px = x + quiet;
                let py = y + quiet;
                path.push_str(&format!("M{px} {py}h1v1h-1z"));
            }
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\"><rect width=\"{size}\" height=\"{size}\" fill=\"#fff\"/><path fill=\"#111\" d=\"{path}\"/></svg>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_title_and_skips_empty() {
        let entries = vec![entry_from(Track {
            path: "/music/sea.mp3".into(),
            title: "Salt".into(),
            artist: "North".into(),
            album: "Harbor".into(),
            album_artist: String::new(),
            track: None,
            disc: None,
            duration_ms: 0,
            folder: "/music".into(),
            replaygain_track: None,
            replaygain_album: None,
        })];
        let catalog = Catalog {
            ready: AtomicBool::new(true),
            entries: Mutex::new(entries),
        };
        assert!(catalog.search("").hits.is_empty());
        assert_eq!(catalog.search("salt").hits.len(), 1);
        assert_eq!(catalog.search("north").hits[0].title, "Salt");
        assert!(catalog.search("missing").hits.is_empty());
        assert!(catalog.search(&"x".repeat(90)).hits.is_empty());
    }

    #[test]
    fn device_name_reads_phone_browsers() {
        assert_eq!(
            device_name(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit Safari/605"
            ),
            "Safari on iPhone"
        );
        assert_eq!(
            device_name(
                "Mozilla/5.0 (Linux; Android 14) AppleWebKit Chrome/120.0.0.0 Mobile Safari/537"
            ),
            "Chrome on Android"
        );
    }

    #[test]
    fn fib_trie_keeps_private_addresses() {
        let text = "\
           |-- 127.0.0.1\n\
              /32 host LOCAL\n\
           |-- 192.168.1.42\n\
              /32 host LOCAL\n\
           |-- 10.0.0.8\n\
              /32 host LOCAL\n\
           |-- 1.2.3.4\n\
              /32 host LOCAL\n";
        let ips = fib_local_ipv4(text);
        assert_eq!(
            ips,
            vec!["192.168.1.42".to_string(), "10.0.0.8".to_string()]
        );
    }

    #[test]
    fn query_decodes_spaces() {
        let pairs = parse_query("q=sea+salt&code=ab%2Fc");
        assert_eq!(pairs[0], ("q".into(), "sea salt".into()));
        assert_eq!(pairs[1], ("code".into(), "ab/c".into()));
    }

    #[test]
    fn qr_is_svg() {
        let svg = qr_svg("http://192.168.1.5:47321/?code=abc123").unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(!svg.contains("abc123"));
        assert!(svg.contains("<path"));
    }

    #[test]
    fn page_does_not_call_the_network() {
        assert!(PAGE.contains("Search library"));
        assert!(!PAGE.to_lowercase().contains("youtube"));
        assert!(!PAGE.contains("https://"));
    }

    #[test]
    fn codes_must_match_exactly() {
        assert!(same_code("abc123", "abc123"));
        assert!(!same_code("abc123", "abc124"));
        assert!(!same_code("abc", "abcd"));
    }
}
