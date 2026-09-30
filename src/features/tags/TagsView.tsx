import { useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, ImagePlus, Save, Search, Trash2, Upload, X } from "lucide-react";
import { applyTrackMeta } from "@/features/player/browse";
import { api, pickAudioFile, pickFolder, pickImageFile, pickSavePath } from "@/lib/api";
import {
  baseName,
  displayTitle,
  errorMessage,
  formatBytes,
  formatTime,
  pictureSrc,
} from "@/lib/format";
import {
  COMMON_FIELDS,
  EMPTY_FIELDS,
  EXTENDED_FIELDS,
  type MediaHit,
  type PictureInfo,
  type TagDoc,
  type TagFields,
} from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";
import { confirm } from "@/ui/confirm";

export function TagsView() {
  const snapshot = useAppStore((state) => state.snapshot);
  const setStatus = useAppStore((state) => state.setStatus);
  const droppedPaths = useAppStore((state) => state.droppedPaths);
  const setDroppedPaths = useAppStore((state) => state.setDroppedPaths);
  const tagFocusPath = useAppStore((state) => state.tagFocusPath);
  const setTagFocusPath = useAppStore((state) => state.setTagFocusPath);
  const [doc, setDoc] = useState<TagDoc | null>(null);
  const [fields, setFields] = useState<TagFields>(EMPTY_FIELDS);
  const [paths, setPaths] = useState<string[]>([]);
  const [apply, setApply] = useState<string[]>([]);
  const [customKey, setCustomKey] = useState("");
  const [customValue, setCustomValue] = useState("");
  const [pictureKind, setPictureKind] = useState("front");
  const [busy, setBusy] = useState(false);
  const [coverHits, setCoverHits] = useState<MediaHit[]>([]);
  const [coverLoading, setCoverLoading] = useState(false);
  const [coverApplying, setCoverApplying] = useState<string | null>(null);
  const coverReq = useRef(0);
  const dropLock = useRef(0);
  const [filesOpen, setFilesOpen] = useState(false);
  const loadPathRef = useRef<(path: string) => Promise<void>>(async () => {});

  const dirty = useMemo(
    () => JSON.stringify(normalize(doc)) !== JSON.stringify(fields),
    [doc, fields],
  );

  async function loadPath(path: string) {
    setBusy(true);
    coverReq.current += 1;
    setCoverHits([]);
    setCoverApplying(null);
    try {
      const next = await api.readTags(path);
      setDoc(next);
      setFields(normalize(next));
      setPaths((current) => (current.includes(path) ? current : [path, ...current]));
      setStatus(null);
    } catch (error) {
      setStatus(errorMessage(error, "Could not read tags"));
    } finally {
      setBusy(false);
    }
  }

  async function searchCovers() {
    if (!doc) return;
    const query = coverQuery(fields, doc.path);
    if (!query) return;
    const gen = ++coverReq.current;
    setCoverLoading(true);
    setStatus(null);
    try {
      const hits = (await api.searchCovers(query)).slice(0, 4);
      if (gen !== coverReq.current) return;
      setCoverHits(hits);
      if (hits.length === 0) setStatus("No artwork found");
    } catch (error) {
      if (gen !== coverReq.current) return;
      setCoverHits([]);
      setStatus(errorMessage(error, "Couldn't find artwork"));
    } finally {
      if (gen === coverReq.current) setCoverLoading(false);
    }
  }

  async function applyCoverHit(hit: MediaHit) {
    if (!doc || !hit.thumbnailUrl) return;
    setCoverApplying(hit.url);
    setBusy(true);
    try {
      const next = await api.addCoverFromUrl(doc.path, hit.thumbnailUrl, pictureKind);
      setDoc(next);
      setStatus("Artwork added", "info");
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't add artwork"));
    } finally {
      setCoverApplying(null);
      setBusy(false);
    }
  }

  loadPathRef.current = loadPath;

  useEffect(() => {
    if (!tagFocusPath) return;
    void loadPathRef.current(tagFocusPath);
    setTagFocusPath(null);
  }, [tagFocusPath, setTagFocusPath]);

  async function openFile() {
    const path = await pickAudioFile("Open file");
    if (path) await loadPath(path);
  }

  async function openFolder() {
    const folder = await pickFolder("Open folder");
    if (!folder) return;
    setBusy(true);
    try {
      const listed = await api.listAudioPaths(folder);
      if (listed.length === 0) {
        setStatus("No audio files in that folder");
        return;
      }
      setPaths(listed);
      await loadPath(listed[0]);
    } catch (error) {
      setStatus(errorMessage(error, "Could not open folder"));
    } finally {
      setBusy(false);
    }
  }

  async function loadDropped(paths: string[]) {
    const unique = [...new Set(paths.map((path) => path.trim()).filter(Boolean))];
    if (unique.length === 0) return;
    setBusy(true);
    try {
      const listed: string[] = [];
      for (const path of unique) {
        listed.push(...(await api.listAudioPaths(path)));
      }
      const next = [...new Set(listed)];
      if (next.length === 0) {
        setStatus("No audio files in that drop");
        return;
      }
      setPaths((current) => {
        const merged = [...current];
        for (const path of next) {
          if (!merged.includes(path)) merged.push(path);
        }
        return merged;
      });
      await loadPath(next[0]);
    } catch (error) {
      setStatus(errorMessage(error, "Could not open dropped files"));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    if (!droppedPaths?.length) return;
    const incoming = droppedPaths;
    setDroppedPaths(null);
    const now = Date.now();
    if (now - dropLock.current < 400) return;
    dropLock.current = now;
    setFilesOpen(false);
    void loadDropped(incoming);
    // loadDropped is a render-local function; the drop payload is the trigger.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [droppedPaths, setDroppedPaths]);

  async function saveCurrent() {
    if (!doc) return;
    setBusy(true);
    try {
      const next = await api.writeTags(doc.path, fields);
      setDoc(next);
      setFields(normalize(next));
      applyTrackMeta(await api.refreshTracks([doc.path]));
      setStatus("Saved tags", "info");
    } catch (error) {
      setStatus(errorMessage(error, "Save failed"));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s") return;
      const panel = document.querySelector("[data-tags-root]");
      if (!panel || panel.closest(".hidden")) return;
      event.preventDefault();
      if (!busy && dirty && doc) void saveCurrent();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  async function saveBatch() {
    if (paths.length === 0 || apply.length === 0) return;
    const labels = new Map<string, string>([...COMMON_FIELDS, ...EXTENDED_FIELDS]);
    const writes: string[] = [];
    const clears: string[] = [];
    for (const key of apply) {
      const label = labels.get(key) ?? key;
      const value = fields[key as keyof TagFields];
      const blank = value === null || value === undefined || String(value).trim() === "";
      (blank ? clears : writes).push(blank ? label : `${label} = “${String(value)}”`);
    }
    const body = [
      `This rewrites the tags in ${paths.length} file${paths.length === 1 ? "" : "s"}. There is no undo.`,
    ];
    if (writes.length) body.push(`Set: ${writes.join(", ")}.`);
    if (clears.length) {
      body.push(`Clear (the field is empty, so it is erased on every file): ${clears.join(", ")}.`);
    }
    const ok = await confirm({
      title: `Apply to ${paths.length} file${paths.length === 1 ? "" : "s"}?`,
      body,
      confirmLabel: clears.length ? "Write and clear" : "Write tags",
      danger: clears.length > 0,
    });
    if (!ok) return;
    setBusy(true);
    try {
      const result = await api.batchWrite(paths, fields, apply);
      applyTrackMeta(await api.refreshTracks(paths));
      if (result.failed.length === 0) {
        setStatus(`Updated ${result.written} file${result.written === 1 ? "" : "s"}`, "info");
      } else {
        const first = result.failed[0];
        setStatus(
          `Updated ${result.written}, failed ${result.failed.length}: ${baseName(first.path)} — ${first.error}`,
        );
      }
      if (doc) await loadPath(doc.path);
    } catch (error) {
      setStatus(errorMessage(error, "Batch save failed"));
    } finally {
      setBusy(false);
    }
  }

  async function addPictureFromDisk() {
    if (!doc) return;
    const imagePath = await pickImageFile();
    if (!imagePath) return;
    setBusy(true);
    try {
      const next = await api.addPicture(doc.path, imagePath, pictureKind);
      setDoc(next);
      applyTrackMeta(await api.refreshTracks([doc.path]));
    } catch (error) {
      setStatus(errorMessage(error, "Could not add artwork"));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section data-tags-root className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-start justify-between gap-3 px-8 pt-6 pb-4">
        <div className="min-w-0">
          <h1 className="text-[28px] font-semibold tracking-tight">Metadata Editor</h1>
          <p className="mt-2 max-w-3xl text-[15px] font-medium leading-6 text-app-muted">
            Edit Common fields, ReplayGain, MusicBrainz IDs, Lyrics, artwork, custom frames. Empty
            fields clear any existing tags, make sure you fill everything necessary out.
          </p>
        </div>
        <button
          type="button"
          className="tags-files-toggle shrink-0 rounded-md border border-app-border px-2 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
          onClick={() => setFilesOpen((open) => !open)}
        >
          <span className="inline-flex items-center gap-1.5">
            <FolderOpen size={15} />
            Files
          </span>
        </button>
      </div>
      <div className="relative flex min-h-0 flex-1">
        {filesOpen ? (
          <button
            type="button"
            aria-label="Close files"
            className="tags-files-scrim absolute inset-0 z-10 bg-black/40"
            onClick={() => setFilesOpen(false)}
          />
        ) : null}
        <div
          className={`tags-files z-20 flex shrink-0 flex-col border-r border-app-line bg-app ${
            filesOpen ? "open" : ""
          }`}
        >
          <div className="flex items-center justify-between px-3 py-2">
            <h2 className="text-[13px] font-semibold uppercase tracking-[0.06em] text-app-muted">
              Files
            </h2>
            <div className="flex gap-1">
              <button
                type="button"
                onClick={() => void openFile()}
                className="rounded-md px-2 py-1 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
              >
                Open file
              </button>
              <button
                type="button"
                onClick={() => void openFolder()}
                className="rounded-md px-2 py-1 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
              >
                Open folder
              </button>
            </div>
          </div>
          {snapshot?.current ? (
            <button
              type="button"
              onClick={() => void loadPath(snapshot.current!.path)}
              className="mx-3 mb-2 rounded-md border border-app-border px-2 py-1.5 text-left text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
            >
              Use current track
            </button>
          ) : null}
          <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
            {paths.length === 0 ? (
              <p className="px-2 text-[14px] font-medium leading-5 text-app-muted">
                Open a file, a folder, or pull the track that is playing.
              </p>
            ) : (
              <ul className="flex flex-col gap-0.5">
                {paths.map((path) => (
                  <li
                    key={path}
                    className="flex items-center gap-1 rounded-md px-1 py-1 hover:bg-app-hover"
                  >
                    <button
                      type="button"
                      onClick={() => {
                        setFilesOpen(false);
                        void loadPath(path);
                      }}
                      className={`min-w-0 flex-1 truncate px-1 text-left text-[14px] font-semibold ${
                        doc?.path === path ? "text-app-text" : "text-app-subtle"
                      }`}
                    >
                      {displayTitle("", path)}
                    </button>
                    <button
                      type="button"
                      title="Remove from list"
                      onClick={() => {
                        const next = paths.filter((item) => item !== path);
                        setPaths(next);
                        if (doc?.path === path) {
                          if (next[0]) void loadPath(next[0]);
                          else {
                            setDoc(null);
                            setFields(EMPTY_FIELDS);
                          }
                        }
                      }}
                      className="rounded p-1 text-app-muted hover:text-app-danger"
                    >
                      <X size={15} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col px-5 py-4">
          {!doc ? (
            <div className="flex min-h-[280px] flex-1 items-center justify-center rounded-xl border-2 border-dashed border-app-border bg-app-raised/60 px-6 py-16 text-center text-[15px] font-medium leading-6 text-app-muted">
              Drop a file or folder on this window
            </div>
          ) : (
            <div className="flex min-h-0 w-full flex-1 flex-col">
              <div className="min-h-0 flex-1 overflow-auto">
                <div className="flex flex-col gap-5 pb-6">
                  <header>
                    <p className="text-[12px] text-app-muted">
                      {doc.format} · {doc.tagType}
                      {doc.sampleRate ? ` · ${doc.sampleRate} Hz` : ""}
                      {doc.bitDepth ? ` · ${doc.bitDepth}-bit` : ""}
                      {doc.bitrateKbps ? ` · ${doc.bitrateKbps} kbps` : ""}
                      {` · ${formatTime(doc.durationMs)}`}
                    </p>
                    <h2 className="mt-1 break-all text-[15px]">{doc.path}</h2>
                  </header>

                  <FieldCard
                    title="Common"
                    fields={COMMON_FIELDS}
                    values={fields}
                    apply={apply}
                    setApply={setApply}
                    onChange={setFields}
                  />
                  <FieldCard
                    title="Extended"
                    fields={EXTENDED_FIELDS}
                    values={fields}
                    apply={apply}
                    setApply={setApply}
                    onChange={setFields}
                  />

                  <section className="rounded-xl border border-app-border bg-app-raised/70 p-4">
                    <h3 className="mb-3 text-[13px] text-app-text">Lyrics</h3>
                    <label className="mb-3 block text-[12px] text-app-muted">
                      Unsynced
                      <textarea
                        value={fields.lyrics}
                        onChange={(event) => setFields({ ...fields, lyrics: event.target.value })}
                        rows={6}
                        className="mt-1 w-full resize-y rounded-md border border-app-border bg-app px-2 py-2 text-[13px] text-app-text"
                      />
                    </label>
                    <label className="block text-[12px] text-app-muted">
                      Synced
                      <textarea
                        value={fields.syncedLyrics}
                        onChange={(event) =>
                          setFields({ ...fields, syncedLyrics: event.target.value })
                        }
                        rows={4}
                        className="mt-1 w-full resize-y rounded-md border border-app-border bg-app px-2 py-2 text-[13px] text-app-text"
                      />
                    </label>
                  </section>

                  <section className="rounded-xl border border-app-border bg-app-raised/70 p-4">
                    <div className="mb-3 flex items-center justify-between">
                      <h3 className="text-[13px] text-app-text">Pictures</h3>
                      <div className="flex items-center gap-2 text-[12px]">
                        <select
                          value={pictureKind}
                          onChange={(event) => setPictureKind(event.target.value)}
                          className="rounded-md border border-app-border bg-app px-2 py-1 text-app-subtle"
                        >
                          <option value="front">Front</option>
                          <option value="back">Back</option>
                          <option value="artist">Artist</option>
                          <option value="leaflet">Leaflet</option>
                          <option value="other">Other</option>
                        </select>
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void addPictureFromDisk()}
                          className="flex cursor-pointer items-center gap-1 rounded-md bg-white/[0.06] px-2 py-1 text-app-subtle disabled:opacity-40"
                        >
                          <ImagePlus size={13} />
                          Add
                        </button>
                        <button
                          type="button"
                          disabled={busy || coverLoading}
                          onClick={() => void searchCovers()}
                          className="flex items-center gap-1 rounded-md bg-white/[0.06] px-2 py-1 text-app-subtle disabled:opacity-40"
                        >
                          <Search size={13} />
                          {coverLoading ? "Searching…" : "Find artwork"}
                        </button>
                      </div>
                    </div>
                    {coverHits.length > 0 ? (
                      <div className="mb-3 grid grid-cols-2 gap-2 sm:grid-cols-4">
                        {coverHits.map((hit) => (
                          <figure
                            key={hit.url}
                            className="overflow-hidden rounded-lg border border-app-border"
                          >
                            {hit.thumbnailUrl ? (
                              <img
                                src={hit.thumbnailUrl}
                                alt=""
                                loading="lazy"
                                decoding="async"
                                className="h-24 w-full object-cover"
                              />
                            ) : (
                              <div className="h-24 bg-app-hover" />
                            )}
                            <figcaption className="flex items-center justify-between gap-1 px-2 py-1.5">
                              <span
                                className="min-w-0 truncate text-[11px] text-app-muted"
                                title={hit.title}
                              >
                                {hit.title}
                              </span>
                              <button
                                type="button"
                                disabled={busy || !hit.thumbnailUrl}
                                onClick={() => void applyCoverHit(hit)}
                                className="shrink-0 text-[11px] font-semibold text-app-accent disabled:opacity-40"
                              >
                                {coverApplying === hit.url ? "Adding…" : "Use"}
                              </button>
                            </figcaption>
                          </figure>
                        ))}
                      </div>
                    ) : null}
                    {doc.pictures.length === 0 ? (
                      <p className="text-[12px] text-app-muted">No embedded pictures.</p>
                    ) : (
                      <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
                        {doc.pictures.map((picture) => (
                          <figure
                            key={`${picture.index}-${picture.size}`}
                            className="overflow-hidden rounded-lg border border-app-border"
                          >
                            <TagPicture path={doc.path} picture={picture} />
                            <figcaption className="flex items-center justify-between px-2 py-1.5 text-[11px] text-app-muted">
                              <span>
                                {picture.kind} · {formatBytes(picture.size)}
                              </span>
                              <span className="flex gap-2">
                                <button
                                  type="button"
                                  onClick={() => {
                                    void (async () => {
                                      const dest = await pickSavePath(`cover-${picture.index}.jpg`);
                                      if (dest)
                                        await api.exportPicture(doc.path, picture.index, dest);
                                    })();
                                  }}
                                >
                                  Export
                                </button>
                                <button
                                  type="button"
                                  onClick={() => {
                                    void (async () => {
                                      const ok = await confirm({
                                        title: "Remove this picture?",
                                        body: "It is deleted from the file's tags.",
                                        confirmLabel: "Remove picture",
                                        danger: true,
                                      });
                                      if (!ok) return;
                                      try {
                                        const next = await api.removePicture(
                                          doc.path,
                                          picture.index,
                                        );
                                        setDoc(next);
                                        applyTrackMeta(await api.refreshTracks([doc.path]));
                                      } catch (error) {
                                        setStatus(errorMessage(error, "Could not remove picture"));
                                      }
                                    })();
                                  }}
                                >
                                  <Trash2 size={12} />
                                </button>
                              </span>
                            </figcaption>
                          </figure>
                        ))}
                      </div>
                    )}
                  </section>

                  <section className="rounded-xl border border-app-border bg-app-raised/70 p-4">
                    <h3 className="mb-3 text-[13px] text-app-text">Advanced / raw frames</h3>
                    <div className="mb-3 flex gap-2">
                      <input
                        value={customKey}
                        onChange={(event) => setCustomKey(event.target.value)}
                        placeholder="Key"
                        className="w-40 rounded-md border border-app-border bg-app px-2 py-1 text-[12px]"
                      />
                      <input
                        value={customValue}
                        onChange={(event) => setCustomValue(event.target.value)}
                        placeholder="Value"
                        className="flex-1 rounded-md border border-app-border bg-app px-2 py-1 text-[12px]"
                      />
                      <button
                        type="button"
                        onClick={() => {
                          void api.addCustomField(doc.path, customKey, customValue).then((next) => {
                            setDoc(next);
                            setCustomKey("");
                            setCustomValue("");
                          });
                        }}
                        className="rounded-md bg-white/[0.06] px-2 py-1 text-[12px] text-app-subtle"
                      >
                        Add
                      </button>
                    </div>
                    <div className="overflow-hidden rounded-md border border-app-border">
                      <table className="w-full text-left text-[12px]">
                        <thead className="bg-white/[0.03] text-app-muted">
                          <tr>
                            <th className="px-2 py-1.5 font-medium">Key</th>
                            <th className="px-2 py-1.5 font-medium">Value</th>
                            <th className="w-16 px-2 py-1.5" />
                          </tr>
                        </thead>
                        <tbody>
                          {doc.raw.map((item) => (
                            <tr
                              key={`${item.key}-${item.value}`}
                              className="border-t border-app-line"
                            >
                              <td className="px-2 py-1.5 text-app-subtle">
                                {item.key}
                                {item.known ? "" : " · custom"}
                              </td>
                              <td className="px-2 py-1.5 break-all text-app-muted">{item.value}</td>
                              <td className="px-2 py-1.5 text-right">
                                <button
                                  type="button"
                                  onClick={() => {
                                    void (async () => {
                                      const ok = await confirm({
                                        title: `Remove “${item.key}”?`,
                                        body: "The frame is removed from this file.",
                                        confirmLabel: "Remove",
                                        danger: true,
                                      });
                                      if (!ok) return;
                                      try {
                                        setDoc(await api.removeCustomField(doc.path, item.key));
                                      } catch (error) {
                                        setStatus(errorMessage(error, "Could not remove field"));
                                      }
                                    })();
                                  }}
                                >
                                  Remove
                                </button>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  </section>
                </div>
              </div>
              <div className="flex shrink-0 items-center justify-between gap-3 border-t border-app-line bg-app px-1 py-3">
                <p
                  className={`text-[13px] font-semibold ${dirty ? "text-app-text" : "text-app-muted"}`}
                >
                  {dirty ? "Unsaved changes" : "Saved"}
                </p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    disabled={busy || paths.length === 0 || apply.length === 0}
                    onClick={() => void saveBatch()}
                    className="flex items-center gap-1.5 rounded-md border border-app-border px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover disabled:opacity-40"
                  >
                    <Upload size={14} />
                    Apply to {paths.length}
                  </button>
                  <button
                    type="button"
                    disabled={busy || !dirty}
                    onClick={() => void saveCurrent()}
                    className="flex items-center gap-1.5 rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg disabled:opacity-40"
                  >
                    <Save size={14} />
                    Save
                  </button>
                </div>
              </div>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}

function FieldCard({
  title,
  fields,
  values,
  apply,
  setApply,
  onChange,
}: {
  title: string;
  fields: Array<[keyof TagFields, string]>;
  values: TagFields;
  apply: string[];
  setApply: (apply: string[]) => void;
  onChange: (fields: TagFields) => void;
}) {
  return (
    <section className="rounded-xl border border-app-border bg-app-raised/70 p-4">
      <h3 className="mb-3 text-[15px] font-semibold text-app-text">{title}</h3>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {fields.map(([key, label]) => (
          <label key={key} className="block text-[13px] font-semibold text-app-muted">
            <span className="mb-1 flex items-center justify-between">
              {label}
              <span className="flex items-center gap-1 text-[10px] uppercase tracking-wide">
                batch
                <input
                  type="checkbox"
                  checked={apply.includes(key)}
                  onChange={(event) => {
                    setApply(
                      event.target.checked ? [...apply, key] : apply.filter((item) => item !== key),
                    );
                  }}
                />
              </span>
            </span>
            <input
              value={values[key]}
              onChange={(event) => onChange({ ...values, [key]: event.target.value })}
              className="w-full rounded-md border border-app-border bg-app px-2 py-1.5 text-[13px] text-app-text"
            />
          </label>
        ))}
      </div>
    </section>
  );
}

function coverQuery(fields: TagFields, path: string): string {
  const title = fields.title.trim();
  const artist = (fields.artists || fields.albumArtist).trim();
  if (title && artist) return `${artist} ${title}`;
  if (title) return title;
  return displayTitle("", path);
}

function TagPicture({ path, picture }: { path: string; picture: PictureInfo }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void api
      .picturePreview(path, picture.index)
      .then((cover) => {
        if (!cancelled && cover) setSrc(pictureSrc(cover.mime, cover.dataBase64));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [path, picture.index, picture.size]);
  return src ? (
    <img src={src} alt={picture.kind} className="h-40 w-full object-cover" />
  ) : (
    <div className="flex h-40 items-center justify-center bg-app-hover text-[11px] text-app-muted">
      Loading…
    </div>
  );
}

function normalize(doc: TagDoc | null): TagFields {
  if (!doc) return { ...EMPTY_FIELDS };
  return {
    ...EMPTY_FIELDS,
    ...Object.fromEntries(Object.entries(doc.fields).map(([key, value]) => [key, value ?? ""])),
  } as TagFields;
}
