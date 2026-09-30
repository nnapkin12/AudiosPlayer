import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { Pencil, Search, X } from "lucide-react";
import { addToNewPlaylist, BrowseBack, LibraryNav, LibraryPage, PlaylistsPage } from "@/features/player/Playlists";
import { PlayPauseIcon } from "@/features/shell/PlayPauseIcon";
import { filterTracks, invalidateBrowse, locateMissing, openBrowsePage } from "@/features/player/browse";
import { DiscoverView } from "@/features/player/DiscoverView";
import { LibraryHome } from "@/features/player/LibraryHome";
import { TrackList } from "@/features/player/TrackList";
import { ContextMenu, type MenuEntry } from "@/features/shell/ContextMenu";
import {
  api,
  pickAudioFile,
  pickAudioFiles,
  pickFolder,
  pickImageFile,
  revealInFiles,
} from "@/lib/api";
import { PlaylistCover, dropPlaylistCover } from "@/lib/covers";
import { baseName, errorMessage } from "@/lib/format";
import type { Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

export function PlayerView() {
  const browse = useAppStore((state) => state.browse);
  const pageTracks = useAppStore((state) => state.pageTracks);
  const pageLoading = useAppStore((state) => state.pageLoading);
  const playlists = useAppStore((state) => state.playlists);
  const currentPath = useAppStore((state) => state.snapshot?.current?.path ?? null);
  const playing = useAppStore((state) => state.snapshot?.playing ?? false);
  const missing = useAppStore((state) => state.missing);
  const setStatus = useAppStore((state) => state.setStatus);
  const setLibraryRoots = useAppStore((state) => state.setLibraryRoots);
  const setPlaylists = useAppStore((state) => state.setPlaylists);
  const [listQuery, setListQuery] = useState("");
  const [renaming, setRenaming] = useState(false);
  const [draftName, setDraftName] = useState("");
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);

  const playlist =
    browse.kind === "playlist" ? playlists.find((item) => item.id === browse.id) : null;
  const currentInPage = Boolean(currentPath && pageTracks.some((track) => track.path === currentPath));
  const showPause = currentInPage && playing;

  const browseKey =
    browse.kind === "folder" ? browse.path : browse.kind === "playlist" ? browse.id : "home";

  useEffect(() => {
    setListQuery("");
    setRenaming(false);
    setDraftName(playlist?.name ?? "");
  }, [browseKey, playlist?.name]);

  const visibleTracks = useMemo(
    () => filterTracks(pageTracks, listQuery),
    [pageTracks, listQuery],
  );
  const missingHere = missing.filter((item) =>
    browse.kind === "playlist"
      ? item.scope === "playlist" && item.id === browse.id
      : browse.kind === "folder"
        ? item.scope === "library" && item.path === browse.path
        : false,
  );

  async function openFile() {
    try {
      const path = await pickAudioFile();
      if (path) useAppStore.getState().applySnapshot(await api.openPath(path));
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't play this"));
    }
  }

  async function addFolder() {
    try {
      const path = await pickFolder();
      if (!path) return;
      const change = await api.addLibraryRoot(path);
      setLibraryRoots(change.roots);
      invalidateBrowse("folder", change.path);
      await openBrowsePage({ kind: "folder", path: change.path }, true);
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't add music"));
    }
  }

  async function playOrToggle() {
    if (currentInPage) {
      try {
        useAppStore.getState().applySnapshot(await api.toggle());
      } catch (error) {
        setStatus(errorMessage(error, "Could not control playback"));
      }
      return;
    }
    await playTracks();
  }

  async function playTracks(startPath?: string) {
    const start =
      startPath ?? (listQuery.trim() && visibleTracks[0] ? visibleTracks[0].path : undefined);
    await playTracksFrom(pageTracks, start);
  }

  async function playTracksFrom(tracks: Track[], startPath?: string) {
    if (tracks.length === 0) return;
    try {
      useAppStore.getState().applySnapshot(await api.playTracks(tracks, startPath));
    } catch (error) {
      setStatus(errorMessage(error, "Could not play"));
    }
  }

  async function commitRename() {
    if (!playlist) return;
    const next = draftName.trim();
    if (!next || next === playlist.name) {
      setDraftName(playlist.name);
      setRenaming(false);
      return;
    }
    try {
      setPlaylists(await api.renamePlaylist(playlist.id, next));
      setRenaming(false);
    } catch (error) {
      setStatus(errorMessage(error, "Could not rename playlist"));
    }
  }

  async function changeCover() {
    if (!playlist) return;
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
    if (!playlist) return;
    try {
      dropPlaylistCover(playlist.id);
      setPlaylists(await api.clearPlaylistCover(playlist.id));
    } catch (error) {
      setStatus(errorMessage(error, "Could not remove playlist picture"));
    }
  }

  function openMenu(event: MouseEvent, items: MenuEntry[]) {
    event.preventDefault();
    event.stopPropagation();
    setMenu({ x: event.clientX, y: event.clientY, items });
  }

  function trackMenu(track: Track, queue: Track[] = pageTracks): MenuEntry[] {
    return [
      { kind: "action", action: { label: "Play", onClick: () => void playTracksFrom(queue, track.path) } },
      {
        kind: "action",
        action: {
          label: "Add to queue",
          onClick: () => {
            void api.enqueuePath(track.path).then((snapshot) => {
              useAppStore.getState().applySnapshot(snapshot);
            }).catch((error) => {
              setStatus(errorMessage(error, "Could not add to queue"));
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
            void addToNewPlaylist([track.path]).catch((error) => {
              setStatus(errorMessage(error, "Could not create playlist"));
            });
          },
        },
        actions: playlists.map((item) => ({
          label: item.name,
          onClick: () => {
            void api.addToPlaylist(item.id, [track.path]).then((list) => {
              setPlaylists(list);
              invalidateBrowse("playlist", item.id);
              if (browse.kind === "playlist" && browse.id === item.id) {
                void openBrowsePage({ kind: "playlist", id: item.id }, true);
              }
            });
          },
        })),
      },
      {
        kind: "action",
        action: {
          label: "Edit metadata",
          onClick: () => {
            useAppStore.getState().setTagFocusPath(track.path);
            useAppStore.getState().setTab("tags");
          },
        },
      },
      {
        kind: "action",
        action: {
          label: "Show in Files",
          onClick: () => {
            void revealInFiles(track.path).catch((error) => {
              setStatus(errorMessage(error, "Couldn't open Files"));
            });
          },
        },
      },
      ...(playlist
        ? ([
            { kind: "sep" },
            {
              kind: "action",
              action: {
                label: "Remove from this playlist",
                danger: true,
                onClick: () => {
                  void api.removeFromPlaylist(playlist.id, track.path).then((list) => {
                    dropPlaylistCover(playlist.id);
                    setPlaylists(list);
                    invalidateBrowse("playlist", playlist.id);
                    void openBrowsePage({ kind: "playlist", id: playlist.id }, true);
                  });
                },
              },
            },
          ] satisfies MenuEntry[])
        : []),
    ];
  }

  const title =
    browse.kind === "home"
      ? "Audios!"
      : browse.kind === "folder"
        ? baseName(browse.path)
        : (playlist?.name ?? "Playlist");

  const filtered = listQuery.trim().length > 0;
  const countLabel = pageLoading
    ? "Reading library…"
    : pageTracks.length === 0
      ? "No songs"
      : filtered
        ? `${visibleTracks.length} of ${pageTracks.length} song${pageTracks.length === 1 ? "" : "s"}`
        : `${pageTracks.length} song${pageTracks.length === 1 ? "" : "s"}`;

  return (
    <section className="flex min-h-0 flex-1">
      <div className="player-nav flex shrink-0 flex-col border-r border-app-line">
        <div className="px-3 pb-2 pt-3">
          <button
            type="button"
            onClick={() => void openBrowsePage({ kind: "home" })}
            className="block text-left text-[28px] font-semibold tracking-tight text-app-text hover:text-app-subtle"
          >
            Music Library
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
          <LibraryNav />
        </div>
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        {browse.kind === "home" ? (
          <LibraryHome />
        ) : browse.kind === "library" ? (
          <LibraryPage
            onMenu={openMenu}
            onAddFile={() => void openFile()}
            onAddFolder={() => void addFolder()}
          />
        ) : browse.kind === "playlists" ? (
          <PlaylistsPage onMenu={openMenu} />
        ) : browse.kind === "discover" || browse.kind === "artist" || browse.kind === "album" ? (
          <DiscoverView
            onPlay={(tracks, startPath) => void playTracksFrom(tracks, startPath)}
            onContext={(event, track, queue) => openMenu(event, trackMenu(track, queue))}
          />
        ) : (
          <>
            <div className="px-5 pt-4">
              {browse.kind === "playlist" && playlist ? (
                <div className="flex flex-col items-center pb-1 text-center">
                  <div className="self-start">
                    <BrowseBack />
                  </div>
                  <button
                    type="button"
                    title={playlist.hasCover ? "Change picture" : "Add picture"}
                    onClick={() => void changeCover()}
                    onContextMenu={(event) =>
                      openMenu(event, [
                        {
                          kind: "action",
                          action: { label: "Change picture", onClick: () => void changeCover() },
                        },
                        ...(playlist.hasCover
                          ? ([
                              {
                                kind: "action",
                                action: {
                                  label: "Remove picture",
                                  onClick: () => void removeCover(),
                                },
                              },
                            ] satisfies MenuEntry[])
                          : []),
                      ])
                    }
                    className="group relative"
                  >
                    <PlaylistCover
                      id={playlist.id}
                      iconSize={48}
                      className="h-48 w-48 rounded-2xl shadow-[0_18px_40px_rgb(0_0_0_/_0.28)]"
                    />
                    <span className="pointer-events-none absolute inset-0 flex items-center justify-center rounded-2xl bg-black/45 text-[13px] font-semibold text-white opacity-0 group-hover:opacity-100">
                      {playlist.hasCover ? "Change" : "Add picture"}
                    </span>
                  </button>
                  <div className="mt-2">
                  <p className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted">
                    Playlist
                  </p>
                  {renaming ? (
                    <form
                      className="mt-1 w-full max-w-md"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void commitRename();
                      }}
                    >
                      <input
                        autoFocus
                        value={draftName}
                        onChange={(event) => setDraftName(event.target.value)}
                        onBlur={() => void commitRename()}
                        className="w-full rounded-md border border-app-border bg-app px-2 py-1 text-[22px] font-semibold text-app-text"
                      />
                    </form>
                  ) : (
                    <div className="mt-1 flex items-center gap-2">
                      <p className="truncate text-[22px] font-semibold text-app-text">{title}</p>
                      <button
                        type="button"
                        title="Rename playlist"
                        onClick={() => {
                          setDraftName(playlist.name);
                          setRenaming(true);
                        }}
                        className="rounded-md p-1 text-app-muted hover:bg-app-hover hover:text-app-text"
                      >
                        <Pencil size={14} />
                      </button>
                    </div>
                  )}
                  <p className="text-[14px] font-medium text-app-muted">{countLabel}</p>
                  </div>
                  <div className="mt-2 flex flex-wrap justify-center gap-2">
                    <button
                      type="button"
                      onClick={() => void pickAudioFilesInto(playlist.id)}
                      className="rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
                    >
                      Add songs
                    </button>
                    <button
                      type="button"
                      onClick={() => void pickFolderInto(playlist.id)}
                      className="rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
                    >
                      Add album
                    </button>
                    <button
                      type="button"
                      onClick={() => void removePlaylist(playlist.id)}
                      className="rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-danger hover:bg-app-hover"
                    >
                      Remove playlist
                    </button>
                    <button
                      type="button"
                      disabled={visibleTracks.length === 0 && !currentInPage}
                      title={showPause ? "Pause" : "Play"}
                      onClick={() => void playOrToggle()}
                      className="flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg disabled:opacity-40"
                    >
                      <PlayPauseIcon playing={showPause} size={14} />
                      {showPause ? "Pause" : "Play"}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="flex flex-wrap items-end justify-between gap-4 pb-4">
                  <div className="min-w-0">
                    <BrowseBack />
                    <p className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted">
                      Library
                    </p>
                    <p className="truncate text-[22px] font-semibold text-app-text">{title}</p>
                    <p className="text-[14px] font-medium text-app-muted">{countLabel}</p>
                  </div>
                  <button
                    type="button"
                    disabled={visibleTracks.length === 0 && !currentInPage}
                    title={showPause ? "Pause" : "Play"}
                    onClick={() => void playOrToggle()}
                    className="flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg disabled:opacity-40"
                  >
                    <PlayPauseIcon playing={showPause} size={14} />
                    {showPause ? "Pause" : "Play"}
                  </button>
                </div>
              )}
            </div>
            {missingHere.length > 0 ? (
              <div className="mx-5 mb-3 flex items-center justify-between gap-3 rounded-xl border border-app-border bg-app-raised px-4 py-3">
                <p className="text-[14px] font-medium leading-5 text-app-text">
                  Can't find {missingHere.length === 1 ? missingHere[0].label : `${missingHere.length} items`}.
                  Point Audios! at the song or album.
                </p>
                <button
                  type="button"
                  onClick={() => {
                    const album = missingHere.find((item) => item.kind === "dir") ?? missingHere[0];
                    void locateMissing(album, missingHere.length > 1 || album.kind === "dir").catch((error) => {
                      setStatus(errorMessage(error, "Couldn't update that path"));
                    });
                  }}
                  className="shrink-0 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
                >
                  Locate
                </button>
              </div>
            ) : null}
            {pageTracks.length > 0 || listQuery ? (
              <ListSearch value={listQuery} onChange={setListQuery} />
            ) : null}
            {pageTracks.length === 0 && !pageLoading ? (
              <div className="px-5">
                <div className="rounded-xl border border-dashed border-app-border bg-app-raised/60 px-4 py-6 text-[14px] font-medium leading-6 text-app-muted">
                  {browse.kind === "playlist"
                    ? "Add songs to this playlist."
                    : "No songs here yet."}
                </div>
              </div>
            ) : visibleTracks.length === 0 && filtered ? (
              <div className="px-5">
                <div className="rounded-xl border border-dashed border-app-border bg-app-raised/60 px-4 py-6 text-[14px] font-medium leading-6 text-app-muted">
                  No songs match that search.
                </div>
              </div>
            ) : (
              <TrackList
                tracks={visibleTracks}
                onPlay={(track) => void playTracks(track.path)}
                onContext={(event, track) => openMenu(event, trackMenu(track))}
              />
            )}
          </>
        )}
      </div>
      {menu ? (
        <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
      ) : null}
    </section>
  );
}

function ListSearch({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="px-5 pb-3">
      <label className="relative block">
        <Search
          size={16}
          className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-app-muted"
        />
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="Search songs"
          className="w-full rounded-lg border border-app-border bg-app-raised py-2.5 pl-9 pr-9 text-[15px] text-app-text"
        />
        {value ? (
          <button
            type="button"
            title="Clear search"
            onClick={() => onChange("")}
            className="absolute right-2 top-1/2 -translate-y-1/2 rounded-md p-1 text-app-muted hover:bg-app-hover hover:text-app-text"
          >
            <X size={14} />
          </button>
        ) : null}
      </label>
    </div>
  );
}

async function removePlaylist(playlistId: string) {
  const list = await api.deletePlaylist(playlistId);
  dropPlaylistCover(playlistId);
  useAppStore.getState().setPlaylists(list);
  await openBrowsePage({ kind: "playlists" });
}

async function pickAudioFilesInto(playlistId: string) {
  const paths = await pickAudioFiles();
  if (paths.length === 0) return;
  const list = await api.addToPlaylist(playlistId, paths);
  dropPlaylistCover(playlistId);
  useAppStore.getState().setPlaylists(list);
  invalidateBrowse("playlist", playlistId);
  await openBrowsePage({ kind: "playlist", id: playlistId }, true);
}

async function pickFolderInto(playlistId: string) {
  const folder = await pickFolder("Add album");
  if (!folder) return;
  const list = await api.addToPlaylist(playlistId, [folder]);
  dropPlaylistCover(playlistId);
  useAppStore.getState().setPlaylists(list);
  invalidateBrowse("playlist", playlistId);
  await openBrowsePage({ kind: "playlist", id: playlistId }, true);
}
