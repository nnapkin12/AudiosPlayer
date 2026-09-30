import { beforeEach, describe, expect, it, vi } from "vitest";

// The store imports the Tauri API boundary and theme side effects. None of
// those are exercised by snapshot handling, so stub them.
vi.mock("@/lib/api", () => ({
  api: { coverArt: vi.fn(async () => null) },
}));
vi.mock("@/lib/theme", () => ({ applyAppearance: vi.fn() }));
vi.mock("@/lib/motion", () => ({ applyMotion: vi.fn() }));

import { useAppStore } from "./useAppStore";
import type { EqState, PlayerSnapshot } from "@/lib/types";

const flatEq: EqState = {
  enabled: false,
  presetId: "flat",
  bands: [],
  preamp: 0,
  autoPreamp: true,
  toneTemplate: false,
  builtins: [],
  customPresets: [],
};

function snapshot(overrides: Partial<PlayerSnapshot> = {}): PlayerSnapshot {
  return {
    current: null,
    index: 0,
    queue: [],
    playing: false,
    positionMs: 0,
    durationMs: 0,
    volume: 1,
    muted: false,
    repeat: "off",
    shuffle: false,
    replaygain: true,
    gapless: true,
    speed: 1,
    sampleRate: 44_100,
    eq: flatEq,
    root: null,
    tree: null,
    error: null,
    ...overrides,
  } as PlayerSnapshot;
}

describe("useAppStore.applySnapshot", () => {
  beforeEach(() => {
    useAppStore.setState({
      snapshot: null,
      status: null,
      error: null,
      positionMs: 0,
      durationMs: 0,
    });
  });

  it("copies position and duration into the tick fields", () => {
    useAppStore.getState().applySnapshot(snapshot({ positionMs: 1234, durationMs: 60_000 }));
    const state = useAppStore.getState();
    expect(state.positionMs).toBe(1234);
    expect(state.durationMs).toBe(60_000);
  });

  it("keeps the EQ builtin catalog from the last full snapshot", () => {
    const builtin = { id: "rock", name: "Rock", description: "", bands: [], preamp: 0 };
    useAppStore.getState().applySnapshot(snapshot({ eq: { ...flatEq, builtins: [builtin] } }));
    // Ticks send an empty catalog to keep IPC small; it must not wipe the UI.
    useAppStore.getState().applySnapshot(snapshot());
    expect(useAppStore.getState().snapshot?.eq?.builtins).toEqual([builtin]);
  });

  it("surfaces a backend error without wiping an info status", () => {
    useAppStore.getState().setStatus("Saved tags", "info");
    useAppStore.getState().applySnapshot(snapshot({ error: "no audio output device" }));
    expect(useAppStore.getState().status).toBe("Saved tags");
    expect(useAppStore.getState().error).toBe("no audio output device");
  });

  it("clears a backend error when the snapshot is healthy", () => {
    useAppStore.getState().applySnapshot(snapshot({ error: "no audio output device" }));
    useAppStore.getState().applySnapshot(snapshot());
    expect(useAppStore.getState().error).toBeNull();
  });

  it("tick keeps the last known duration when a tick reports zero", () => {
    useAppStore.getState().applySnapshot(snapshot({ durationMs: 90_000 }));
    useAppStore.getState().applyTick({ positionMs: 500, durationMs: 0 });
    expect(useAppStore.getState().durationMs).toBe(90_000);
    expect(useAppStore.getState().positionMs).toBe(500);
  });
});
