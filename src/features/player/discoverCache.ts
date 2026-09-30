import type { Track } from "@/lib/types";

let tracks: Track[] | null = null;
let source = "";

export function peekDiscover(sourceKey: string): Track[] | null {
  return source === sourceKey ? tracks : null;
}

export function setDiscover(sourceKey: string, next: Track[]): void {
  source = sourceKey;
  tracks = next;
}

export function dropDiscoverTracks(): void {
  tracks = null;
  source = "";
}
