import { describe, expect, it } from "vitest";
import {
  applyToneControls,
  clampEqGain,
  editBand,
  effectivePreamp,
  eqStatus,
  isToneTemplate,
  normalizeEqBands,
  magnitudeDb,
  newCustomEq,
  setMacroGain,
  toneBands,
  EMPTY_EQ,
  type EqBand,
} from "./eq";

describe("tone template", () => {
  it("locks bass as a low shelf and air as a high shelf", () => {
    const bands = toneBands();
    expect(isToneTemplate(bands)).toBe(true);
    expect(bands[0].kind).toBe("lowShelf");
    expect(bands[9].kind).toBe("highShelf");
    expect(bands[1].gain).toBe(0);
  });

  it("treats a hidden boost as a full profile", () => {
    const bands = toneBands();
    bands[5].gain = 3;
    expect(isToneTemplate(bands)).toBe(false);
  });
});

describe("macro gain", () => {
  it("keeps shelf type while the lock is on", () => {
    const next = setMacroGain(toneBands(), 0, 4, true);
    expect(next[0].kind).toBe("lowShelf");
    expect(next[0].freq).toBe(90);
    expect(next[0].gain).toBe(4);
  });

  it("does not rewrite an imported band when the lock is off", () => {
    const bands = toneBands();
    bands[0] = { kind: "peak", freq: 105, q: 0.7, gain: 2 };
    const next = setMacroGain(bands, 0, 3, false);
    expect(next[0].kind).toBe("peak");
    expect(next[0].freq).toBe(105);
    expect(next[0].gain).toBe(3);
  });
});

describe("editBand", () => {
  it("switches a locked shelf to peaking when frequency is edited", () => {
    const next = editBand(toneBands(), 0, { freq: 120 });
    expect(next[0].kind).toBe("peak");
    expect(next[0].freq).toBe(120);
  });

  it("keeps an explicit shelf choice", () => {
    const next = editBand(toneBands(), 0, { kind: "lowShelf", freq: 120 });
    expect(next[0].kind).toBe("lowShelf");
    expect(next[0].freq).toBe(120);
  });
});

describe("applyToneControls", () => {
  it("zeros hidden bands and restores shelves", () => {
    const bands: EqBand[] = toneBands().map((band, index) => ({
      ...band,
      kind: "peak",
      freq: 1000 + index,
      gain: 1,
    }));
    const next = applyToneControls(bands);
    expect(isToneTemplate(next)).toBe(true);
    expect(next[0].kind).toBe("lowShelf");
    expect(next[0].gain).toBe(1);
    expect(next[1].gain).toBe(0);
    expect(next[9].kind).toBe("highShelf");
  });
});

describe("magnitude", () => {
  it("hits the peaking gain at the center frequency", () => {
    const bands = toneBands();
    bands[4] = { kind: "peak", freq: 1000, q: Math.SQRT2, gain: 6 };
    const db = magnitudeDb(bands, 0, 44100, 1000);
    expect(Math.abs(db - 6)).toBeLessThan(0.05);
  });
});

describe("effectivePreamp", () => {
  it("turns the mix down from the summed curve", () => {
    const bands = toneBands();
    bands[2] = { kind: "peak", freq: 1000, q: 1, gain: 6 };
    bands[4] = { kind: "peak", freq: 1000, q: 1, gain: 6 };
    expect(effectivePreamp(bands, 0, true)).toBeLessThan(-6);
    expect(effectivePreamp(bands, 0, false)).toBe(0);
  });
});

describe("eqStatus", () => {
  it("shows off when disabled", () => {
    expect(eqStatus(EMPTY_EQ)).toBe("Off");
  });
});

describe("newCustomEq", () => {
  it("copies the live bands", () => {
    const bands = toneBands();
    bands[0].gain = 3;
    const preset = newCustomEq({ ...EMPTY_EQ, bands }, "Desk");
    expect(preset.name).toBe("Desk");
    expect(preset.id.startsWith("custom-")).toBe(true);
    expect(preset.bands[0].gain).toBe(3);
    expect(preset.bands[0].kind).toBe("lowShelf");
  });
});

describe("shelf Q", () => {
  it("stores a shelf Q above 1 as 1", () => {
    const bands = toneBands();
    bands[0].q = 4;
    const stored = normalizeEqBands(bands);
    expect(stored[0].q).toBe(1);
    expect(stored[0].kind).toBe("lowShelf");
  });
});

describe("clampEqGain", () => {
  it("keeps the slider range", () => {
    expect(clampEqGain(0)).toBe(0);
    expect(clampEqGain(99)).toBe(12);
    expect(clampEqGain(-99)).toBe(-12);
  });
});
