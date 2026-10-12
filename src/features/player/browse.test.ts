import { describe, expect, it } from "vitest";
import {
  BROWSE_HISTORY_CAP,
  keepsDiscoverCatalog,
  pushBrowsePast,
} from "@/features/player/browse";
import type { BrowsePage } from "@/lib/types";

describe("browse history", () => {
  it("keeps the back stack at the cap", () => {
    const past: BrowsePage[] = [];
    for (let index = 0; index < BROWSE_HISTORY_CAP + 8; index += 1) {
      pushBrowsePast(past, { kind: "folder", path: `/music/${index}` });
    }
    expect(past).toHaveLength(BROWSE_HISTORY_CAP);
    expect(past[0]).toEqual({ kind: "folder", path: "/music/8" });
    expect(past[past.length - 1]).toEqual({
      kind: "folder",
      path: `/music/${BROWSE_HISTORY_CAP + 7}`,
    });
  });

  it("holds the discover catalog only while those pages are open", () => {
    expect(keepsDiscoverCatalog({ kind: "discover" })).toBe(true);
    expect(keepsDiscoverCatalog({ kind: "artist", name: "Ada" })).toBe(true);
    expect(keepsDiscoverCatalog({ kind: "album", artist: "Ada", album: "One" })).toBe(true);
    expect(keepsDiscoverCatalog({ kind: "home" })).toBe(false);
    expect(keepsDiscoverCatalog({ kind: "library" })).toBe(false);
    expect(keepsDiscoverCatalog({ kind: "folder", path: "/music" })).toBe(false);
  });
});
