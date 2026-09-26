import { useEffect, useState } from "react";
import { api, listen } from "@/lib/api";
import { errorMessage } from "@/lib/format";
import type { RemoteStatus } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

const IDLE: RemoteStatus = {
  running: false,
  url: null,
  urls: [],
  code: null,
  qrSvg: null,
  indexing: false,
  songs: 0,
  clients: [],
};

export function RemotePanel() {
  const setStatus = useAppStore((state) => state.setStatus);
  const [remote, setRemote] = useState<RemoteStatus>(IDLE);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let alive = true;
    let unlisten: () => void = () => undefined;
    void api.remoteStatus().then((status) => {
      if (alive) setRemote(status);
    }).catch(() => undefined);
    void listen<RemoteStatus>("remote://status", (status) => {
      if (alive) setRemote(status);
    }).then((stop) => {
      if (alive) unlisten = stop;
      else stop();
    });
    return () => {
      alive = false;
      unlisten();
    };
  }, []);

  async function start() {
    setBusy(true);
    setCopied(false);
    try {
      setRemote(await api.remoteStart());
    } catch (error) {
      setStatus(errorMessage(error, "Could not start the web remote"));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    setCopied(false);
    try {
      setRemote(await api.remoteStop());
    } catch (error) {
      setStatus(errorMessage(error, "Could not stop the web remote"));
    } finally {
      setBusy(false);
    }
  }

  async function copy(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }

  if (!remote.running) {
    return (
      <div className="flex flex-col items-start gap-3">
        <h2 className="text-[13px] font-semibold text-app-muted">Audios! web</h2>
        <p className="text-[14px] text-app-muted">
          Control this computer from a phone on the same Wi-Fi. Music still plays here.
        </p>
        <button
          type="button"
          disabled={busy}
          onClick={() => void start()}
          className="rounded-md border border-app-border px-3 py-1.5 text-[14px] font-semibold text-app-text hover:bg-app-hover disabled:opacity-50"
        >
          Start web remote
        </button>
      </div>
    );
  }

  const url = remote.url;
  const others = remote.urls.filter((item) => item !== url);

  return (
    <div className="flex flex-col items-start gap-4">
      <h2 className="text-[13px] font-semibold text-app-muted">Audios! web</h2>
      <div>
        <p className="text-[15px] font-semibold text-app-accent">Live</p>
        <p className="mt-1 text-[14px] text-app-muted">
          {remote.indexing
            ? "Reading the library…"
            : remote.songs === 0
              ? "No songs in the library yet."
              : `${remote.songs.toLocaleString()} songs`}
        </p>
      </div>

      {url ? (
        <div className="flex w-full flex-col gap-2">
          <input
            readOnly
            value={url}
            aria-label="Remote link"
            onFocus={(event) => event.target.select()}
            className="w-full rounded-md border border-app-border bg-transparent px-3 py-2 text-[14px] text-app-text"
          />
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => void copy(url)}
              className="rounded-md border border-app-border px-3 py-1.5 text-[14px] font-semibold text-app-text hover:bg-app-hover"
            >
              {copied ? "Copied" : "Copy"}
            </button>
            {remote.code ? (
              <span className="text-[13px] text-app-muted">Code {remote.code}</span>
            ) : null}
          </div>
          {remote.qrSvg ? (
            <img
              alt="QR code for the remote link"
              className="h-40 w-40 bg-white p-2"
              src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(remote.qrSvg)}`}
            />
          ) : null}
          {others.length > 0 ? (
            <ul className="flex flex-col gap-1">
              {others.map((item) => (
                <li key={item} className="break-all text-[13px] text-app-muted">
                  {item}
                </li>
              ))}
            </ul>
          ) : null}
        </div>
      ) : (
        <p className="text-[14px] text-app-muted">
          No LAN address found. The port is open, but this computer has no private IPv4 address.
        </p>
      )}

      {remote.clients.length === 0 ? (
        <p className="text-[14px] text-app-muted">No phone connected yet.</p>
      ) : (
        <ul className="flex w-full flex-col gap-2">
          {remote.clients.map((client) => (
            <li key={client.id} className="flex items-baseline justify-between gap-3 text-[14px]">
              <span className="text-app-text">{client.name}</span>
              <span className={client.connected ? "text-app-accent" : "text-app-muted"}>
                {client.connected ? "connected" : "last seen"}
              </span>
            </li>
          ))}
        </ul>
      )}

      <p className="text-[13px] text-app-muted">
        If the phone cannot open the link, allow the port in the firewall.
      </p>
      <button
        type="button"
        disabled={busy}
        onClick={() => void stop()}
        className="rounded-md border border-app-border px-3 py-1.5 text-[14px] font-semibold text-app-text hover:bg-app-hover disabled:opacity-50"
      >
        Stop web remote
      </button>
    </div>
  );
}
