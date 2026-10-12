import { useRef } from "react";
import { api } from "@/lib/api";
import { errorMessage } from "@/lib/format";
import type { RepeatMode, Track } from "@/lib/types";
import { useAppStore } from "@/store/useAppStore";

export function nextRepeat(mode: RepeatMode): RepeatMode {
  if (mode === "off") return "all";
  if (mode === "all") return "one";
  return "off";
}

export function repeatLabel(mode: RepeatMode): string {
  if (mode === "all") return "Repeat all";
  if (mode === "one") return "Repeat one";
  return "Repeat off";
}

function setPlaying(playing: boolean) {
  const snapshot = useAppStore.getState().snapshot;
  if (!snapshot || snapshot.playing === playing) return;
  useAppStore.setState({ snapshot: { ...snapshot, playing } });
}

/**
 * One in-flight play/skip at a time. Errors go to the status line.
 * Commands already emit `player://state`, so callers must not applySnapshot.
 */
export function useTransport() {
  const exclusive = useRef(false);

  async function run(op: () => Promise<unknown>, lock = true): Promise<void> {
    if (lock) {
      if (exclusive.current) return;
      exclusive.current = true;
    }
    try {
      await op();
    } catch (error) {
      useAppStore.getState().setStatus(errorMessage(error, "Playback failed"));
    } finally {
      if (lock) exclusive.current = false;
    }
  }

  return {
    toggle: () =>
      void run(async () => {
        const before = useAppStore.getState().snapshot?.playing ?? false;
        setPlaying(!before);
        try {
          await api.toggle();
        } catch (error) {
          setPlaying(before);
          throw error;
        }
      }),
    next: () => void run(() => api.next()),
    previous: () => void run(() => api.previous()),
    seek: (ms: number) => void run(() => api.seek(ms), false),
    playTracks: (tracks: Track[], startPath?: string) =>
      void run(() => api.playTracks(tracks, startPath)),
    setShuffle: (shuffle: boolean) => void run(() => api.setShuffle(shuffle), false),
    setRepeat: (repeat: RepeatMode) => void run(() => api.setRepeat(repeat), false),
    setMuted: (muted: boolean) => void run(() => api.setMuted(muted), false),
    setVolume: (volume: number) => void run(() => api.setVolume(volume), false),
  };
}
