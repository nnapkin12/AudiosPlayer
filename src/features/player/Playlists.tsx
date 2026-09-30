import { useEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { ArrowLeft, Compass, ListMusic, MoreHorizontal, Music, Plus } from "lucide-react";
import {
  goBack,
  invalidateBrowse,
  locateMissing,
  openBrowsePage,
  samePage,
} from "@/features/player/browse";
import { api, pickAudioFiles, pickFolder, pickImageFile, revealInFiles } from "@/lib/api";
import { CoverPicture, PlaylistCover, dropPlaylistCover, useNearView } from "@/lib/covers";
import { baseName, errorMessage } from "@/lib/format";
import type { Playlist } from "@/lib/types";
import type { MenuEntry } from "@/features/shell/ContextMenu";
import { useAppStore } from "@/store/useAppStore";
import { confirm } from "@/ui/confirm";

export function BrowseBack() {
  const canGoBack = useAppStore((state) => state.canGoBack);
  if (!canGoBack) return null;
  return (
    <button
      type="button"
      title="Back"
      aria-label="Back"
      onClick={() => void goBack()}
      className="mb-3 flex h-9 w-9 items-center justify-center rounded-full text-app-text hover:bg-app-hover"
    >
      <ArrowLeft size={20} />
    </button>
  );
}

export function LibraryNav() {
  const browse = useAppStore((state) => state.browse);
  const libraryActive = browse.kind === "library" || browse.kind === "folder";
  const playlistActive = browse.kind === "playlists" || browse.kind === "playlist";
  const discoverActive =
    browse.kind === "discover" || browse.kind === "artist" || browse.kind === "album";

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-1">
      <NavEntry
        active={browse.kind === "home"}
        label="Audios!"
        icon={<img src="/audios.png" alt="" className="h-8 w-8 rounded-md bg-white object-cover" />}
        onClick={() => void openBrowsePage({ kind: "home" })}
      />
      <NavEntry
        active={discoverActive}
        label="Discover"
        icon={<Compass size={18} className="text-app-muted" />}
        onClick={() => void openBrowsePage({ kind: "discover" })}
      />
      <NavEntry
        active={libraryActive}
        label="Library"
        icon={<Music size={18} className="text-app-muted" />}
        onClick={() => void openBrowsePage({ kind: "library" })}
      />
      <NavEntry
        active={playlistActive}
        label="Playlists"
        icon={<ListMusic size={18} className="text-app-muted" />}
        onClick={() => void openBrowsePage({ kind: "playlists" })}
      />
    </div>
  );
}

function NavEntry({
  active,
  label,
  icon,
  onClick,
}: {
  active: boolean;
  label: string;
  icon: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`flex items-center gap-2 rounded-lg px-2 py-2 text-left ${
        active ? "bg-app-hover" : "hover:bg-app-hover"
      }`}
    >
      <span className="flex h-8 w-8 shrink-0 items-center justify-center">{icon}</span>
      <span className="nav-label truncate text-[15px] font-semibold">{label}</span>
    </button>
  );
}

export function LibraryPage({
  onMenu,
  onAddFile,
  onAddFolder,
}: {
  onMenu: (event: MouseEvent, items: MenuEntry[]) => void;
  onAddFile: () => void;
  onAddFolder: () => void;
}) {
  const libraryRoots = useAppStore((state) => state.libraryRoots);
  const missing = useAppStore((state) => state.missing);
  const setLibraryRoots = useAppStore((state) => state.setLibraryRoots);
  const setStatus = useAppStore((state) => state.setStatus);

  async function removeRoot(path: string) {
    try {
      const roots = await api.removeLibraryRoot(path);
      setLibraryRoots(roots);
      const browse = useAppStore.getState().browse;
      if (browse.kind === "folder" && browse.path === path) {
        await openBrowsePage({ kind: "library" });
      }
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't remove this"));
    }
  }

  return (
    <div className="min-h-0 flex-1 overflow-auto px-6 py-6">
      <BrowseBack />
      <div className="flex flex-wrap items-end justify-between gap-4">
        <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Library</h1>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onAddFile}
            className="rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
          >
            Play a song
          </button>
          <button
            type="button"
            onClick={onAddFolder}
            className="rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
          >
            Add music
          </button>
        </div>
      </div>
      {libraryRoots.length === 0 ? (
        <p className="mt-6 max-w-lg text-[15px] font-medium leading-6 text-app-muted">
          Add a folder of audio files. Each folder shows up here with its artwork.
        </p>
      ) : (
        <div className="cover-grid mt-6">
          {libraryRoots.map((path) => {
            const gone = missing.some((item) => item.scope === "library" && item.path === path);
            const items = folderMenu(path, removeRoot, setStatus);
            return (
              <div key={path} className="relative">
                <button
                  type="button"
                  onClick={() => void openBrowsePage({ kind: "folder", path })}
                  onContextMenu={(event) => onMenu(event, items)}
                  className="w-full rounded-2xl p-3 text-center hover:bg-app-hover"
                >
                  <FolderArt path={path} />
                  <span
                    className={`mt-3 block truncate text-[16px] font-semibold ${gone ? "text-app-danger" : "text-app-text"}`}
                  >
                    {baseName(path)}
                    {gone ? " missing" : ""}
                  </span>
                </button>
                <button
                  type="button"
                  title="Folder menu"
                  aria-label="Folder menu"
                  onClick={(event) => {
                    event.stopPropagation();
                    onMenu(event, items);
                  }}
                  className="absolute right-3 top-3 flex h-8 w-8 items-center justify-center rounded-md bg-black/35 text-white hover:bg-black/50"
                >
                  <MoreHorizontal size={16} />
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function FolderArt({ path }: { path: string }) {
  const near = useNearView();
  const [paths, setPaths] = useState<string[] | null>(null);

  useEffect(() => {
    if (!near.ready) return;
    let cancelled = false;
    setPaths(null);
    void api
      .scanTracks(path, true)
      .then((tracks) => {
        if (!cancelled) setPaths(tracks.slice(0, 4).map((track) => track.path));
      })
      .catch(() => {
        if (!cancelled) setPaths([]);
      });
    return () => {
      cancelled = true;
    };
  }, [path, near.ready]);

  if (!near.ready || !paths || paths.length === 0) {
    return (
      <div
        ref={near.ref}
        className="aspect-square w-full rounded-xl bg-gradient-to-br from-app-hover to-app"
      />
    );
  }
  if (paths.length === 1) {
    return <CoverPicture path={paths[0]} className="aspect-square h-auto w-full rounded-xl" />;
  }
  return (
    <div className="grid aspect-square w-full grid-cols-2 grid-rows-2 overflow-hidden rounded-xl">
      {Array.from({ length: 4 }, (_, index) => paths[index] ?? paths[index % paths.length]).map(
        (file, index) => (
          <CoverPicture
            key={`${file}:${index}`}
            path={file}
            className="h-full w-full rounded-none"
          />
        ),
      )}
    </div>
  );
}

export function PlaylistsPage({
  onMenu,
}: {
  onMenu: (event: MouseEvent, items: MenuEntry[]) => void;
}) {
  const playlists = useAppStore((state) => state.playlists);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const setPlaylists = useAppStore((state) => state.setPlaylists);
  const setStatus = useAppStore((state) => state.setStatus);

  async function create() {
    try {
      const list = await api.createPlaylist(name.trim());
      setPlaylists(list);
      setName("");
      setCreating(false);
      const created = list[list.length - 1];
      if (created) await openBrowsePage({ kind: "playlist", id: created.id });
    } catch (error) {
      setStatus(errorMessage(error, "Could not create playlist"));
    }
  }

  return (
    <div className="min-h-0 flex-1 overflow-auto px-6 py-6">
      <BrowseBack />
      <div className="flex flex-wrap items-end justify-between gap-4">
        <h1 className="text-[28px] font-semibold tracking-tight text-app-text">Playlists</h1>
        <button
          type="button"
          onClick={() => setCreating(true)}
          className="flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
        >
          <Plus size={14} />
          New playlist
        </button>
      </div>
      {creating ? (
        <form
          className="mt-4 max-w-sm"
          onSubmit={(event) => {
            event.preventDefault();
            void create();
          }}
        >
          <input
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            onBlur={() => {
              if (!name.trim()) setCreating(false);
            }}
            placeholder="Playlist name"
            className="w-full rounded-lg border border-app-border bg-app-raised px-3 py-2.5 text-[15px]"
          />
        </form>
      ) : null}
      {playlists.length === 0 && !creating ? (
        <p className="mt-6 max-w-lg text-[15px] font-medium leading-6 text-app-muted">
          Make a playlist, then add songs or a folder to it.
        </p>
      ) : (
        <div className="cover-grid mt-6">
          {playlists.map((playlist) => (
            <PlaylistCard key={playlist.id} playlist={playlist} onMenu={onMenu} />
          ))}
        </div>
      )}
    </div>
  );
}

function PlaylistCard({
  playlist,
  onMenu,
}: {
  playlist: Playlist;
  onMenu: (event: MouseEvent, items: MenuEntry[]) => void;
}) {
  const setPlaylists = useAppStore((state) => state.setPlaylists);
  const setStatus = useAppStore((state) => state.setStatus);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(playlist.name);
  const renameLock = useRef(false);

  async function playFrom() {
    try {
      useAppStore.getState().applySnapshot(await api.playPlaylist(playlist.id));
    } catch (error) {
      setStatus(errorMessage(error, "Could not play playlist"));
    }
  }

  async function addFiles() {
    try {
      const paths = await pickAudioFiles();
      if (paths.length === 0) return;
      setPlaylists(await api.addToPlaylist(playlist.id, paths));
      refreshPlaylist(playlist.id);
    } catch (error) {
      setStatus(errorMessage(error, "Could not add songs"));
    }
  }

  async function addFolder() {
    try {
      const folder = await pickFolder("Add album");
      if (!folder) return;
      setPlaylists(await api.addToPlaylist(playlist.id, [folder]));
      refreshPlaylist(playlist.id);
    } catch (error) {
      setStatus(errorMessage(error, "Could not add album"));
    }
  }

  async function commitRename() {
    const next = draft.trim();
    if (renameLock.current) return;
    if (!next || next === playlist.name) {
      setDraft(playlist.name);
      setRenaming(false);
      return;
    }
    renameLock.current = true;
    try {
      setPlaylists(await api.renamePlaylist(playlist.id, next));
      setRenaming(false);
    } catch (error) {
      setStatus(errorMessage(error, "Could not rename playlist"));
    } finally {
      renameLock.current = false;
    }
  }

  async function changeCover() {
    const path = await pickImageFile();
    if (!path) return;
    try {
      dropPlaylistCover(playlist.id);
      setPlaylists(await api.setPlaylistCover(playlist.id, path));
    } catch (error) {
      setStatus(errorMessage(error, "Could not set playlist picture"));
    }
  }

  async function removeCover() {
    try {
      dropPlaylistCover(playlist.id);
      setPlaylists(await api.clearPlaylistCover(playlist.id));
    } catch (error) {
      setStatus(errorMessage(error, "Could not remove playlist picture"));
    }
  }

  async function remove() {
    const count = playlist.items.length;
    const ok = await confirm({
      title: `Delete “${playlist.name.trim() || "Untitled"}”?`,
      body: [
        count > 0
          ? `The playlist and its ${count} item${count === 1 ? "" : "s"} are removed from Audios!.`
          : "The playlist is removed from Audios!.",
        "Your music files are not touched.",
      ],
      confirmLabel: "Delete playlist",
      danger: true,
    });
    if (!ok) return;
    try {
      dropPlaylistCover(playlist.id);
      setPlaylists(await api.deletePlaylist(playlist.id));
      const browse = useAppStore.getState().browse;
      if (browse.kind === "playlist" && browse.id === playlist.id) {
        await openBrowsePage({ kind: "playlists" });
      }
    } catch (error) {
      setStatus(errorMessage(error, "Could not delete playlist"));
    }
  }

  const cardMenu: MenuEntry[] = [
    {
      kind: "action",
      action: {
        label: "Open",
        onClick: () => void openBrowsePage({ kind: "playlist", id: playlist.id }),
      },
    },
    { kind: "action", action: { label: "Play", onClick: () => void playFrom() } },
    {
      kind: "action",
      action: {
        label: "Rename",
        onClick: () => {
          setDraft(playlist.name);
          setRenaming(true);
        },
      },
    },
    {
      kind: "action",
      action: { label: "Change picture", onClick: () => void changeCover() },
    },
    ...(playlist.hasCover
      ? ([
          {
            kind: "action",
            action: { label: "Remove picture", onClick: () => void removeCover() },
          },
        ] satisfies MenuEntry[])
      : []),
    { kind: "action", action: { label: "Add songs", onClick: () => void addFiles() } },
    { kind: "action", action: { label: "Add album", onClick: () => void addFolder() } },
    { kind: "sep" },
    {
      kind: "action",
      action: { label: "Delete playlist", danger: true, onClick: () => void remove() },
    },
  ];

  return (
    <div className="relative rounded-2xl p-3 text-center hover:bg-app-hover">
      {renaming ? (
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void commitRename();
          }}
        >
          <PlaylistCover
            id={playlist.id}
            iconSize={56}
            className="aspect-square h-auto w-full rounded-xl"
          />
          <input
            autoFocus
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={() => void commitRename()}
            className="mt-3 w-full rounded-md border border-app-border bg-app px-2 py-1 text-center text-[16px] font-semibold"
          />
        </form>
      ) : (
        <button
          type="button"
          onClick={() => void openBrowsePage({ kind: "playlist", id: playlist.id })}
          onContextMenu={(event) => onMenu(event, cardMenu)}
          className="w-full text-center"
        >
          <PlaylistCover
            id={playlist.id}
            iconSize={56}
            className="aspect-square h-auto w-full rounded-xl"
          />
          <span className="mt-3 block truncate text-[16px] font-semibold text-app-text">
            {playlist.name}
          </span>
          <span className="block truncate text-[14px] font-medium text-app-muted">
            {playlist.items.length} item{playlist.items.length === 1 ? "" : "s"}
          </span>
        </button>
      )}
      {renaming ? null : (
        <button
          type="button"
          title="Playlist menu"
          aria-label="Playlist menu"
          onClick={(event) => {
            event.stopPropagation();
            onMenu(event, cardMenu);
          }}
          className="absolute right-3 top-3 flex h-8 w-8 items-center justify-center rounded-md bg-black/35 text-white hover:bg-black/50"
        >
          <MoreHorizontal size={16} />
        </button>
      )}
    </div>
  );
}

export async function addToNewPlaylist(paths: string[]) {
  const created = await api.createPlaylist("");
  useAppStore.getState().setPlaylists(created);
  const playlist = created[created.length - 1];
  if (!playlist) {
    throw new Error("Could not create playlist");
  }
  const list = await api.addToPlaylist(playlist.id, paths);
  useAppStore.getState().setPlaylists(list);
  refreshPlaylist(playlist.id);
}

function folderMenu(
  path: string,
  removeRoot: (path: string) => Promise<void>,
  setStatus: (status: string | null) => void,
): MenuEntry[] {
  const playlists = useAppStore.getState().playlists;
  const gone = useAppStore
    .getState()
    .missing.some((item) => item.scope === "library" && item.path === path);
  return [
    ...(gone
      ? ([
          {
            kind: "action",
            action: {
              label: "Locate album",
              onClick: () => {
                void locateMissing({
                  scope: "library",
                  id: path,
                  path,
                  kind: "dir",
                  label: baseName(path),
                }).catch((error) => {
                  setStatus(errorMessage(error, "Couldn't update that path"));
                });
              },
            },
          },
        ] satisfies MenuEntry[])
      : []),
    {
      kind: "action",
      action: {
        label: "Open",
        onClick: () => void openBrowsePage({ kind: "folder", path }),
      },
    },
    {
      kind: "action",
      action: {
        label: "Play",
        onClick: () => {
          void api
            .playQueuePaths([path])
            .then((snapshot) => {
              useAppStore.getState().applySnapshot(snapshot);
            })
            .catch((error) => {
              setStatus(errorMessage(error, "Couldn't play this"));
            });
        },
      },
    },
    {
      kind: "submenu",
      label: "Add to playlist",
      pinned: {
        label: "New playlist",
        onClick: () => {
          void addToNewPlaylist([path]).catch((error) => {
            setStatus(errorMessage(error, "Could not create playlist"));
          });
        },
      },
      actions: playlists.map((playlist) => ({
        label: playlist.name,
        onClick: () => {
          void api.addToPlaylist(playlist.id, [path]).then((list) => {
            useAppStore.getState().setPlaylists(list);
            refreshPlaylist(playlist.id);
          });
        },
      })),
    },
    {
      kind: "action",
      action: {
        label: "Show in Files",
        onClick: () => {
          void revealInFiles(path).catch((error) => {
            setStatus(errorMessage(error, "Couldn't open Files"));
          });
        },
      },
    },
    { kind: "sep" },
    {
      kind: "action",
      action: { label: "Remove from library", danger: true, onClick: () => void removeRoot(path) },
    },
  ];
}

function refreshPlaylist(playlistId: string) {
  dropPlaylistCover(playlistId);
  invalidateBrowse("playlist", playlistId);
  const browse = useAppStore.getState().browse;
  if (samePage(browse, { kind: "playlist", id: playlistId })) {
    void openBrowsePage({ kind: "playlist", id: playlistId }, true);
  }
}
