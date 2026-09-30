export const EQ_BAND_COUNT = 10;
export const EQ_GAIN_MIN = -12;
export const EQ_GAIN_MAX = 12;
export const EQ_Q_MIN = 0.1;
export const EQ_Q_MAX = 10;
export const EQ_FREQ_MIN = 20;
export const EQ_FREQ_MAX = 20000;
export const MAX_USER_EQ_PRESETS = 20;
export const WORKING_EQ_ID = "custom";
export const FLAT_EQ_ID = "flat";

export type FilterKind = "peak" | "lowShelf" | "highShelf";

export interface EqBand {
  kind: FilterKind;
  freq: number;
  q: number;
  gain: number;
}

export interface MacroSlot {
  index: number;
  label: string;
  hint: string;
  kind: FilterKind;
  freq: number;
  q: number;
}

/** Same locks as the Rust engine. Bass and treble are shelves until advanced edits them. */
export const MACRO_SLOTS: MacroSlot[] = [
  { index: 0, label: "Bass", hint: "Low shelf", kind: "lowShelf", freq: 90, q: 0.71 },
  { index: 2, label: "Low", hint: "Warmth", kind: "peak", freq: 300, q: 1 },
  { index: 4, label: "Mid", hint: "Body", kind: "peak", freq: 1000, q: 1 },
  { index: 7, label: "Presence", hint: "Clarity", kind: "peak", freq: 3500, q: 1 },
  { index: 9, label: "Air", hint: "High shelf", kind: "highShelf", freq: 10000, q: 0.71 },
];

const SPARE_FREQS = [160, 600, 2000, 6000, 14000];

export interface EqUserPreset {
  id: string;
  name: string;
  bands: EqBand[];
  preamp: number;
  autoPreamp: boolean;
}

export interface EqBuiltin {
  id: string;
  name: string;
  description: string;
  bands: EqBand[];
  preamp: number;
}

export interface EqState {
  enabled: boolean;
  presetId: string;
  bands: EqBand[];
  preamp: number;
  autoPreamp: boolean;
  toneTemplate: boolean;
  customPresets: EqUserPreset[];
  builtins: EqBuiltin[];
}

export interface EqUpdate {
  enabled: boolean;
  presetId: string;
  bands: EqBand[];
  preamp: number;
  autoPreamp: boolean;
}

export function toneBands(): EqBand[] {
  const bands: EqBand[] = Array.from({ length: EQ_BAND_COUNT }, () => ({
    kind: "peak" as FilterKind,
    freq: 1000,
    q: 1,
    gain: 0,
  }));
  let spare = 0;
  bands.forEach((band, index) => {
    const lock = MACRO_SLOTS.find((slot) => slot.index === index);
    if (lock) {
      band.kind = lock.kind;
      band.freq = lock.freq;
      band.q = lock.q;
    } else {
      band.freq = SPARE_FREQS[spare] ?? 1000;
      spare += 1;
    }
  });
  return bands;
}

export const EMPTY_EQ: EqState = {
  enabled: false,
  presetId: FLAT_EQ_ID,
  bands: toneBands(),
  preamp: 0,
  autoPreamp: true,
  toneTemplate: true,
  customPresets: [],
  builtins: [],
};

export function clampEqGain(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(EQ_GAIN_MAX, Math.max(EQ_GAIN_MIN, value));
}

export function clampPreamp(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(12, Math.max(-30, value));
}

export function clampEqFreq(value: number): number {
  if (!Number.isFinite(value)) return 1000;
  return Math.min(EQ_FREQ_MAX, Math.max(EQ_FREQ_MIN, value));
}

export function clampEqQ(value: number): number {
  if (!Number.isFinite(value)) return 1;
  return Math.min(EQ_Q_MAX, Math.max(EQ_Q_MIN, value));
}

export function normalizeEqBands(input: EqBand[] | undefined): EqBand[] {
  const out = toneBands();
  (input ?? []).slice(0, EQ_BAND_COUNT).forEach((band, index) => {
    out[index] = {
      kind: band?.kind === "lowShelf" || band?.kind === "highShelf" ? band.kind : "peak",
      freq: clampEqFreq(band?.freq),
      q:
        band?.kind === "lowShelf" || band?.kind === "highShelf"
          ? Math.min(1, clampEqQ(band?.q))
          : clampEqQ(band?.q),
      gain: clampEqGain(band?.gain),
    };
  });
  return out;
}

export function isToneTemplate(bands: EqBand[]): boolean {
  const normalized = normalizeEqBands(bands);
  for (const slot of MACRO_SLOTS) {
    const band = normalized[slot.index];
    if (
      !band ||
      band.kind !== slot.kind ||
      Math.abs(band.freq - slot.freq) > 0.05 ||
      Math.abs(band.q - slot.q) > 0.001
    ) {
      return false;
    }
  }
  return normalized.every(
    (band, index) => MACRO_SLOTS.some((slot) => slot.index === index) || Math.abs(band.gain) < 0.01,
  );
}

export function setMacroGain(
  bands: EqBand[],
  index: number,
  gain: number,
  enforceLock: boolean,
): EqBand[] {
  const next = normalizeEqBands(bands);
  const slot = MACRO_SLOTS.find((item) => item.index === index);
  const band = next[index];
  if (!slot || !band) return next;
  if (enforceLock) {
    band.kind = slot.kind;
    band.freq = slot.freq;
    band.q = slot.q;
  }
  band.gain = clampEqGain(gain);
  return next;
}

/** Drop hidden bands and lock bass to a low shelf and treble to a high shelf. */
export function applyToneControls(bands: EqBand[]): EqBand[] {
  const next = toneBands();
  for (const slot of MACRO_SLOTS) {
    next[slot.index].gain = clampEqGain(bands[slot.index]?.gain ?? 0);
  }
  return next;
}

/**
 * Editing frequency or Q on a locked shelf unlocks it to a peaking band.
 * Choosing a type in the menu keeps that type.
 */
export function editBand(bands: EqBand[], index: number, patch: Partial<EqBand>): EqBand[] {
  const next = normalizeEqBands(bands);
  const band = next[index];
  if (!band) return next;
  const slot = MACRO_SLOTS.find((item) => item.index === index);
  const lockedShelf =
    slot &&
    (slot.kind === "lowShelf" || slot.kind === "highShelf") &&
    band.kind === slot.kind &&
    Math.abs(band.freq - slot.freq) <= 0.5 &&
    Math.abs(band.q - slot.q) <= 0.02;
  const unlocking = patch.kind === undefined && (patch.freq !== undefined || patch.q !== undefined);
  if (lockedShelf && unlocking) band.kind = "peak";
  if (patch.kind) band.kind = patch.kind;
  if (patch.freq !== undefined) band.freq = clampEqFreq(patch.freq);
  if (patch.q !== undefined) band.q = clampEqQ(patch.q);
  if (patch.gain !== undefined) band.gain = clampEqGain(patch.gain);
  return next;
}

function designMagnitudeDb(band: EqBand, sampleRate: number, freq: number): number {
  if (Math.abs(band.gain) < 0.01) return 0;
  if (freq <= 0 || freq >= sampleRate * 0.49) return 0;
  const a = 10 ** (band.gain / 40);
  const w0 = (2 * Math.PI * band.freq) / sampleRate;
  const cosW = Math.cos(w0);
  const sinW = Math.sin(w0);
  let b0: number;
  let b1: number;
  let b2: number;
  let a0: number;
  let a1: number;
  let a2: number;
  if (band.kind === "peak") {
    const alpha = sinW / (2 * band.q);
    b0 = 1 + alpha * a;
    b1 = -2 * cosW;
    b2 = 1 - alpha * a;
    a0 = 1 + alpha / a;
    a1 = -2 * cosW;
    a2 = 1 - alpha / a;
  } else {
    const shelfQ = Math.min(1, Math.max(0.1, band.q));
    const inner = (a + 1 / a) * (1 / shelfQ - 1) + 2;
    if (inner < 0) return 0;
    const alpha = (sinW / 2) * Math.sqrt(inner);
    const sqrtA = Math.sqrt(a);
    if (band.kind === "lowShelf") {
      b0 = a * (a + 1 - (a - 1) * cosW + 2 * sqrtA * alpha);
      b1 = 2 * a * (a - 1 - (a + 1) * cosW);
      b2 = a * (a + 1 - (a - 1) * cosW - 2 * sqrtA * alpha);
      a0 = a + 1 + (a - 1) * cosW + 2 * sqrtA * alpha;
      a1 = -2 * (a - 1 + (a + 1) * cosW);
      a2 = a + 1 + (a - 1) * cosW - 2 * sqrtA * alpha;
    } else {
      b0 = a * (a + 1 + (a - 1) * cosW + 2 * sqrtA * alpha);
      b1 = -2 * a * (a - 1 + (a + 1) * cosW);
      b2 = a * (a + 1 + (a - 1) * cosW - 2 * sqrtA * alpha);
      a0 = a + 1 - (a - 1) * cosW + 2 * sqrtA * alpha;
      a1 = 2 * (a - 1 - (a + 1) * cosW);
      a2 = a + 1 - (a - 1) * cosW - 2 * sqrtA * alpha;
    }
  }
  if (!Number.isFinite(a0) || Math.abs(a0) < 1e-12) return 0;
  const inv = 1 / a0;
  const w = (2 * Math.PI * freq) / sampleRate;
  const z1Re = Math.cos(w);
  const z1Im = -Math.sin(w);
  const z2Re = z1Re * z1Re - z1Im * z1Im;
  const z2Im = 2 * z1Re * z1Im;
  const numRe = b0 * inv + b1 * inv * z1Re + b2 * inv * z2Re;
  const numIm = b1 * inv * z1Im + b2 * inv * z2Im;
  const denRe = 1 + a1 * inv * z1Re + a2 * inv * z2Re;
  const denIm = a1 * inv * z1Im + a2 * inv * z2Im;
  const num = Math.hypot(numRe, numIm);
  const den = Math.hypot(denRe, denIm);
  if (den < 1e-20) return 0;
  return 20 * Math.log10(num / den);
}

export function magnitudeDb(
  bands: EqBand[],
  preampDb: number,
  sampleRate: number,
  freq: number,
): number {
  return normalizeEqBands(bands).reduce(
    (sum, band) => sum + designMagnitudeDb(band, sampleRate, freq),
    preampDb,
  );
}

export function compositePeakDb(bands: EqBand[], sampleRate = 48000): number {
  const nyquist = sampleRate * 0.45;
  const freqs: number[] = [];
  for (let step = 0; step < 256; step += 1) {
    const freq = 20 * 1000 ** (step / 255);
    if (freq < nyquist) freqs.push(freq);
  }
  for (const band of normalizeEqBands(bands)) {
    if (Math.abs(band.gain) < 0.01) continue;
    const half = Math.max(1, band.freq / Math.max(0.1, band.q));
    for (let step = 0; step < 9; step += 1) {
      const freq = band.freq - half + (half * 2 * step) / 8;
      if (freq > 20 && freq < nyquist) freqs.push(freq);
    }
  }
  return freqs.reduce((peak, freq) => Math.max(peak, magnitudeDb(bands, 0, sampleRate, freq)), 0);
}

export function effectivePreamp(
  bands: EqBand[],
  preamp: number,
  autoPreamp: boolean,
  sampleRate = 48000,
): number {
  const user = clampPreamp(preamp);
  if (!autoPreamp) return user;
  const peak = compositePeakDb(bands, sampleRate);
  if (peak > 0) return Math.max(-48, Math.min(user, -peak));
  return user;
}

export function responsePoints(
  bands: EqBand[],
  preamp: number,
  autoPreamp: boolean,
  sampleRate = 48000,
): Array<{ f: number; db: number }> {
  const level = effectivePreamp(bands, preamp, autoPreamp, sampleRate);
  const points = [];
  const nyquist = sampleRate * 0.45;
  for (let step = 0; step < 80; step += 1) {
    const f = 20 * 1000 ** (step / 79);
    if (f >= nyquist) break;
    points.push({ f, db: magnitudeDb(bands, level, sampleRate, f) });
  }
  return points;
}

export function eqPresetName(eq: EqState): string {
  if (eq.presetId === WORKING_EQ_ID) return "Custom";
  const builtin = eq.builtins.find((item) => item.id === eq.presetId);
  if (builtin) return builtin.name;
  const custom = eq.customPresets.find((item) => item.id === eq.presetId);
  if (custom) return custom.name;
  return eq.enabled ? "Custom" : "Flat";
}

export function eqStatus(eq: EqState): string {
  if (!eq.enabled) return "Off";
  return eqPresetName(eq);
}

export function toEqUpdate(eq: EqState): EqUpdate {
  return {
    enabled: eq.enabled,
    presetId: eq.presetId,
    bands: normalizeEqBands(eq.bands),
    preamp: clampPreamp(eq.preamp),
    autoPreamp: eq.autoPreamp,
  };
}

export function newCustomEq(eq: EqState, name = "Custom EQ"): EqUserPreset {
  const id =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? `custom-${crypto.randomUUID()}`
      : `custom-${Date.now()}`;
  return {
    id,
    name,
    bands: normalizeEqBands(eq.bands),
    preamp: clampPreamp(eq.preamp),
    autoPreamp: eq.autoPreamp,
  };
}

export function dirtyEqPreset(eq: EqState): EqState {
  const bands = normalizeEqBands(eq.bands);
  if (eq.presetId === WORKING_EQ_ID) return { ...eq, bands, toneTemplate: isToneTemplate(bands) };
  return { ...eq, bands, presetId: WORKING_EQ_ID, toneTemplate: isToneTemplate(bands) };
}

export function formatHz(freq: number): string {
  if (freq >= 1000) {
    const khz = freq / 1000;
    return `${khz >= 10 ? khz.toFixed(0) : khz.toFixed(1)} kHz`;
  }
  return `${Math.round(freq)} Hz`;
}
