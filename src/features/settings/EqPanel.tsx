import { useEffect, useMemo, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import { api } from "@/lib/api";
import {
  EQ_FREQ_MAX,
  EQ_FREQ_MIN,
  EQ_GAIN_MAX,
  EQ_GAIN_MIN,
  EQ_Q_MAX,
  EQ_Q_MIN,
  FLAT_EQ_ID,
  MACRO_SLOTS,
  MAX_USER_EQ_PRESETS,
  WORKING_EQ_ID,
  applyToneControls,
  dirtyEqPreset,
  editBand,
  effectivePreamp,
  eqStatus,
  formatHz,
  isToneTemplate,
  newCustomEq,
  normalizeEqBands,
  responsePoints,
  setMacroGain,
  toEqUpdate,
  toneBands,
  EMPTY_EQ,
  type EqBand,
  type EqBuiltin,
  type EqState,
  type EqUserPreset,
  type FilterKind,
} from "@/lib/eq";
import { errorMessage } from "@/lib/format";
import { useAppStore } from "@/store/useAppStore";
import { confirm } from "@/ui/confirm";

const KIND_OPTIONS: Array<{ id: FilterKind; label: string }> = [
  { id: "peak", label: "Peak" },
  { id: "lowShelf", label: "Low shelf" },
  { id: "highShelf", label: "High shelf" },
];

export function EqPanel() {
  const snapshot = useAppStore((state) => state.snapshot);
  const applySnapshot = useAppStore((state) => state.applySnapshot);
  const setStatus = useAppStore((state) => state.setStatus);
  const remote = snapshot?.eq ?? EMPTY_EQ;
  const [eq, setEq] = useState<EqState>(remote);
  const [advanced, setAdvanced] = useState(false);
  const [selected, setSelected] = useState(0);
  const [name, setName] = useState("Custom EQ");
  const [paste, setPaste] = useState("");
  const dragging = useRef(false);
  const sourceId = useRef(remote.presetId || FLAT_EQ_ID);
  const timer = useRef(0);

  const remoteKey = [
    remote.enabled,
    remote.presetId,
    remote.preamp,
    remote.autoPreamp,
    ...(remote.bands ?? []).map((band) => `${band.kind}:${band.freq}:${band.gain}:${band.q}`),
  ].join("|");

  useEffect(() => {
    if (!dragging.current) {
      setEq({
        ...remote,
        bands: normalizeEqBands(remote.bands),
        toneTemplate: remote.toneTemplate ?? isToneTemplate(remote.bands ?? []),
      });
      if (remote.presetId && remote.presetId !== WORKING_EQ_ID) {
        sourceId.current = remote.presetId;
        const custom = remote.customPresets.find((item) => item.id === remote.presetId);
        if (custom) setName(custom.name);
      }
    }
  }, [remoteKey]); // eslint-disable-line react-hooks/exhaustive-deps -- remoteKey is the snapshot equality check

  useEffect(() => () => window.clearTimeout(timer.current), []);

  function push(next: EqState, immediate = false) {
    const bands = normalizeEqBands(next.bands);
    const state = { ...next, bands, toneTemplate: isToneTemplate(bands) };
    setEq(state);
    window.clearTimeout(timer.current);
    const send = () => {
      void api
        .setEq(toEqUpdate(state))
        .then(applySnapshot)
        .catch((error) => setStatus(errorMessage(error, "Couldn't save equalizer")));
    };
    if (immediate) send();
    else timer.current = window.setTimeout(send, 80);
  }

  function pickBuiltin(item: EqBuiltin) {
    sourceId.current = item.id;
    dragging.current = false;
    push(
      {
        ...eq,
        enabled: true,
        presetId: item.id,
        bands: normalizeEqBands(item.bands),
        preamp: item.preamp,
      },
      true,
    );
  }

  function pickUser(item: EqUserPreset) {
    sourceId.current = item.id;
    setName(item.name);
    dragging.current = false;
    push(
      {
        ...eq,
        enabled: true,
        presetId: item.id,
        bands: normalizeEqBands(item.bands),
        preamp: item.preamp,
        autoPreamp: item.autoPreamp,
      },
      true,
    );
  }

  function tweak(patch: Partial<EqState>) {
    push(dirtyEqPreset({ ...eq, ...patch }));
  }

  function resetSource() {
    const builtin = eq.builtins.find((item) => item.id === sourceId.current);
    if (builtin) {
      pickBuiltin(builtin);
      return;
    }
    const custom = eq.customPresets.find((item) => item.id === sourceId.current);
    if (custom) pickUser(custom);
  }

  function resetFlat() {
    const flat = eq.builtins.find((item) => item.id === FLAT_EQ_ID);
    if (flat) pickBuiltin(flat);
    else {
      sourceId.current = FLAT_EQ_ID;
      push({ ...eq, presetId: FLAT_EQ_ID, bands: toneBands(), preamp: 0 }, true);
    }
  }

  async function savePreset() {
    const existing = eq.customPresets.find((item) => item.id === sourceId.current);
    const preset = existing
      ? {
          ...existing,
          name: name.trim() || existing.name,
          bands: normalizeEqBands(eq.bands),
          preamp: eq.preamp,
          autoPreamp: eq.autoPreamp,
        }
      : newCustomEq(eq, name.trim() || "Custom EQ");
    try {
      const next = await api.saveCustomEq(preset);
      sourceId.current = preset.id;
      dragging.current = false;
      applySnapshot(next);
      setName(preset.name);
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't save preset"));
    }
  }

  async function removePreset(id: string) {
    const preset = eq.customPresets.find((item) => item.id === id);
    const ok = await confirm({
      title: `Remove preset “${preset?.name ?? "Custom"}”?`,
      body: "The saved curve is gone. The equalizer keeps its current settings.",
      confirmLabel: "Remove preset",
      danger: true,
    });
    if (!ok) return;
    try {
      applySnapshot(await api.deleteCustomEq(id));
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't remove preset"));
    }
  }

  async function importProfile() {
    try {
      const next = await api.importParametricEq(paste);
      dragging.current = false;
      sourceId.current = WORKING_EQ_ID;
      applySnapshot(next);
      setPaste("");
      setStatus("Loaded parametric profile");
    } catch (error) {
      setStatus(errorMessage(error, "Couldn't read that profile"));
    }
  }

  const sampleRate = snapshot?.sampleRate && snapshot.sampleRate > 0 ? snapshot.sampleRate : 48000;
  const audible = eq.bands.some((band) => Math.abs(band.gain) >= 0.01);
  const showProfileNote = !advanced && audible && !eq.toneTemplate;
  const autoPre = effectivePreamp(eq.bands, eq.preamp, eq.autoPreamp, sampleRate);
  const curve = useMemo(
    () => responsePoints(eq.bands, eq.preamp, eq.autoPreamp, sampleRate),
    [eq.bands, eq.preamp, eq.autoPreamp, sampleRate],
  );
  const sourceName =
    eq.builtins.find((item) => item.id === sourceId.current)?.name ??
    eq.customPresets.find((item) => item.id === sourceId.current)?.name ??
    "preset";
  const dirty = eq.presetId === WORKING_EQ_ID;

  return (
    <div className="flex flex-col gap-5">
      <div className="flex items-center justify-between gap-4">
        <h3 className="text-[16px] font-semibold text-app-text">Equalizer</h3>
        <p className="text-[13px] font-semibold text-app-subtle">{eqStatus(eq)}</p>
      </div>

      <ResponseCurve
        points={curve}
        bands={eq.bands}
        selected={advanced ? selected : null}
        onSelect={(index) => {
          setSelected(index);
          setAdvanced(true);
        }}
      />

      <Toggle
        label="Enable"
        checked={eq.enabled}
        onChange={(enabled) => push({ ...eq, enabled }, true)}
      />

      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {eq.builtins.map((item) => (
          <button
            key={item.id}
            type="button"
            title={item.description}
            onClick={() => pickBuiltin(item)}
            className={`rounded-lg border px-3 py-2.5 text-left text-[14px] font-semibold leading-5 ${
              eq.presetId === item.id
                ? "border-app-accent bg-app-hover text-app-text"
                : "border-app-border text-app-subtle hover:bg-app-hover"
            }`}
          >
            {item.name}
          </button>
        ))}
        {eq.customPresets.map((item) => (
          <div key={item.id} className="relative">
            <button
              type="button"
              title={item.name}
              onClick={() => pickUser(item)}
              className={`w-full rounded-lg border px-3 py-2.5 pr-8 text-left text-[14px] font-semibold leading-5 ${
                eq.presetId === item.id
                  ? "border-app-accent bg-app-hover text-app-text"
                  : "border-app-border text-app-subtle hover:bg-app-hover"
              }`}
            >
              {item.name}
            </button>
            <button
              type="button"
              title="Remove"
              aria-label={`Remove ${item.name}`}
              onClick={() => void removePreset(item.id)}
              className="absolute right-1.5 top-1.5 rounded p-1 text-app-muted hover:text-app-text"
            >
              <X size={14} />
            </button>
          </div>
        ))}
        <button
          type="button"
          title={eq.customPresets.length >= MAX_USER_EQ_PRESETS ? "Limit reached" : "Save preset"}
          aria-label="Save preset"
          disabled={eq.customPresets.length >= MAX_USER_EQ_PRESETS}
          onClick={() => void savePreset()}
          className="flex items-center justify-center rounded-lg border border-dashed border-app-border px-3 py-2.5 text-app-muted hover:border-app-accent hover:bg-app-hover hover:text-app-text disabled:opacity-40"
        >
          <Plus size={20} />
        </button>
      </div>

      <label className="flex cursor-pointer items-center justify-between gap-4">
        <span className="text-[16px] font-semibold text-app-text">Advanced</span>
        <input
          type="checkbox"
          checked={advanced}
          onChange={(event) => setAdvanced(event.target.checked)}
          className="h-4 w-4"
        />
      </label>

      {showProfileNote ? (
        <div className="rounded-lg border border-app-border bg-app-hover px-3 py-3">
          <p className="text-[14px] leading-6 text-app-text">
            A full parametric profile is active. These sliders only change gain. The other bands are
            still in the signal.
          </p>
          <button
            type="button"
            onClick={() => tweak({ bands: applyToneControls(eq.bands) })}
            className="mt-2 text-[14px] font-semibold text-app-accent hover:underline"
          >
            Use tone controls only
          </button>
        </div>
      ) : null}

      {advanced ? (
        <div className="flex flex-col gap-4">
          <div className="grid grid-cols-5 gap-2">
            {eq.bands.map((band, index) => {
              const active = Math.abs(band.gain) >= 0.05;
              const current = index === selected;
              return (
                <button
                  key={index}
                  type="button"
                  onClick={() => setSelected(index)}
                  className={`rounded-lg border px-2 py-2 text-left ${
                    current
                      ? "border-app-accent bg-app-hover text-app-text"
                      : "border-app-border text-app-subtle hover:bg-app-hover"
                  }`}
                >
                  <span className="block text-[13px] font-semibold">{index + 1}</span>
                  <span className="block truncate text-[12px]">{formatHz(band.freq)}</span>
                  <span
                    className={`block text-[12px] font-semibold tabular-nums ${active ? "text-app-text" : "text-app-muted"}`}
                  >
                    {band.gain > 0 ? "+" : ""}
                    {band.gain.toFixed(1)}
                  </span>
                </button>
              );
            })}
          </div>
          {eq.bands[selected] ? (
            <BandEditor
              index={selected}
              band={eq.bands[selected]}
              onChange={(patch) => tweak({ bands: editBand(eq.bands, selected, patch) })}
              onHold={(hold) => {
                dragging.current = hold;
              }}
            />
          ) : null}
          <details className="rounded-lg border border-app-border px-3 py-2">
            <summary className="cursor-pointer text-[15px] font-semibold text-app-text">
              Paste AutoEQ
            </summary>
            <p className="mt-2 text-[13px] leading-5 text-app-muted">
              ParametricEQ lines only. Preamp, PK, LSC, and HSC. Lines starting with # are ignored.
            </p>
            <textarea
              value={paste}
              onChange={(event) => setPaste(event.target.value)}
              rows={4}
              spellCheck={false}
              className="mt-2 w-full rounded-lg border border-app-border bg-app px-3 py-2 text-[13px] leading-5 text-app-text"
            />
            <button
              type="button"
              onClick={() => void importProfile()}
              className="mt-2 rounded-lg border border-app-border px-3 py-2 text-[14px] font-semibold text-app-subtle hover:bg-app-hover"
            >
              Load profile
            </button>
          </details>
        </div>
      ) : (
        <div className="flex flex-col gap-4">
          {MACRO_SLOTS.map((slot) => (
            <GainSlider
              key={slot.index}
              label={slot.label}
              hint={`${slot.hint} · ${formatHz(eq.bands[slot.index]?.freq ?? slot.freq)}`}
              value={eq.bands[slot.index]?.gain ?? 0}
              onChange={(gain) =>
                tweak({
                  bands: setMacroGain(eq.bands, slot.index, gain, false),
                })
              }
              onHold={(hold) => {
                dragging.current = hold;
              }}
            />
          ))}
        </div>
      )}

      <GainSlider
        label="Level"
        value={eq.preamp}
        min={-30}
        onChange={(preamp) => tweak({ preamp })}
        onHold={(hold) => {
          dragging.current = hold;
        }}
      />
      {eq.autoPreamp && autoPre < eq.preamp - 0.05 ? (
        <p className="text-[13px] text-app-muted">Adjusted to prevent clipping.</p>
      ) : null}

      <Toggle
        label="Prevent clipping"
        hint="Lowers the level when the combined curve would distort."
        checked={eq.autoPreamp}
        onChange={(autoPreamp) => push(dirtyEqPreset({ ...eq, autoPreamp }), true)}
      />

      <button
        type="button"
        onClick={resetFlat}
        className="self-start rounded-lg border border-app-border px-3 py-2 text-[14px] font-semibold text-app-subtle hover:bg-app-hover"
      >
        Reset
      </button>

      {dirty && sourceId.current !== WORKING_EQ_ID ? (
        <button
          type="button"
          onClick={resetSource}
          className="self-start text-[14px] font-semibold text-app-accent hover:underline"
        >
          Reset to {sourceName}
        </button>
      ) : null}

      <form
        className="flex flex-col gap-2 sm:flex-row sm:items-center"
        onSubmit={(event) => {
          event.preventDefault();
          void savePreset();
        }}
      >
        <input
          value={name}
          onChange={(event) => setName(event.target.value)}
          placeholder="Name"
          className="min-w-0 flex-1 rounded-lg border border-app-border bg-app px-3 py-2 text-[15px] text-app-text"
        />
        <button
          type="submit"
          className="rounded-lg bg-app-play px-3 py-2 text-[14px] font-semibold text-app-play-fg"
        >
          Save
        </button>
      </form>
    </div>
  );
}

function ResponseCurve({
  points,
  bands,
  selected,
  onSelect,
}: {
  points: Array<{ f: number; db: number }>;
  bands: EqBand[];
  selected: number | null;
  onSelect: (index: number) => void;
}) {
  const width = 320;
  const height = 140;
  const pad = 12;
  const minDb = -12;
  const maxDb = 12;
  const xOf = (freq: number) =>
    pad + (Math.log(Math.max(20, freq) / 20) / Math.log(1000)) * (width - pad * 2);
  const yOf = (db: number) =>
    pad + ((maxDb - Math.max(minDb, Math.min(maxDb, db))) / (maxDb - minDb)) * (height - pad * 2);
  const coords = points.map((point) => `${xOf(point.f).toFixed(1)},${yOf(point.db).toFixed(1)}`);
  const zeroY = yOf(0);
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      className="h-36 w-full"
      role="img"
      aria-label="Equalizer frequency response"
    >
      <line
        x1={pad}
        x2={width - pad}
        y1={zeroY}
        y2={zeroY}
        className="stroke-app-border"
        strokeWidth="1"
      />
      <polyline
        fill="none"
        className="stroke-app-accent"
        strokeWidth="2"
        points={coords.join(" ")}
      />
      {bands.map((band, index) => (
        <circle
          key={index}
          cx={xOf(band.freq)}
          cy={yOf(band.gain)}
          r={selected === index ? 6 : 4}
          className={selected === index ? "fill-app-accent" : "fill-app-text"}
          opacity={Math.abs(band.gain) >= 0.05 || selected === index ? 1 : 0.35}
          onClick={() => onSelect(index)}
        >
          <title>{`Band ${index + 1}, ${formatHz(band.freq)}`}</title>
        </circle>
      ))}
    </svg>
  );
}

function BandEditor({
  index,
  band,
  onChange,
  onHold,
}: {
  index: number;
  band: EqBand;
  onChange: (patch: Partial<EqBand>) => void;
  onHold: (hold: boolean) => void;
}) {
  return (
    <div className="flex flex-col gap-3 border-t border-app-line pt-4">
      <div className="flex items-center justify-between gap-3">
        <span className="text-[15px] font-semibold text-app-text">Band {index + 1}</span>
        <select
          aria-label={`Band ${index + 1} type`}
          value={band.kind}
          onChange={(event) => onChange({ kind: event.target.value as FilterKind })}
          className="rounded-lg border border-app-border bg-app px-2 py-1 text-[14px] text-app-text"
        >
          {KIND_OPTIONS.map((option) => (
            <option key={option.id} value={option.id}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
      <label className="flex items-center justify-between gap-3">
        <span className="text-[14px] font-semibold text-app-text">Frequency</span>
        <span className="flex items-center gap-2 text-[13px] text-app-subtle">
          <input
            type="number"
            min={EQ_FREQ_MIN}
            max={EQ_FREQ_MAX}
            step={1}
            value={Math.round(band.freq)}
            aria-label={`Band ${index + 1} frequency`}
            onChange={(event) => onChange({ freq: Number(event.target.value) })}
            className="w-24 rounded-lg border border-app-border bg-app px-2 py-1 text-right text-[14px] text-app-text"
          />
          Hz
        </span>
      </label>
      <GainSlider
        label="Gain"
        value={band.gain}
        onChange={(gain) => onChange({ gain })}
        onHold={onHold}
      />
      <label className="block">
        <span className="flex items-center justify-between gap-3">
          <span className="text-[14px] font-semibold text-app-text">Q</span>
          <span className="text-[13px] font-semibold tabular-nums text-app-subtle">
            {band.q.toFixed(2)}
          </span>
        </span>
        <input
          type="range"
          min={EQ_Q_MIN}
          max={band.kind === "peak" ? EQ_Q_MAX : 1}
          step={0.01}
          value={band.q}
          aria-label={`Band ${index + 1} Q`}
          onPointerDown={() => onHold(true)}
          onPointerUp={() => onHold(false)}
          onPointerCancel={() => onHold(false)}
          onChange={(event) => onChange({ q: Number(event.target.value) })}
          className="mt-2 w-full"
        />
      </label>
    </div>
  );
}

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-start justify-between gap-4">
      <span>
        <span className="block text-[16px] font-semibold text-app-text">{label}</span>
        {hint ? (
          <span className="mt-1 block text-[14px] leading-6 text-app-muted">{hint}</span>
        ) : null}
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="mt-1 h-4 w-4"
      />
    </label>
  );
}

function GainSlider({
  label,
  hint,
  value,
  min = EQ_GAIN_MIN,
  max = EQ_GAIN_MAX,
  onChange,
  onHold,
}: {
  label: string;
  hint?: string;
  value: number;
  min?: number;
  max?: number;
  onChange: (value: number) => void;
  onHold: (hold: boolean) => void;
}) {
  return (
    <label className="block">
      <span className="flex items-start justify-between gap-3">
        <span>
          <span className="block text-[15px] font-semibold text-app-text">{label}</span>
          {hint ? (
            <span className="mt-1 block text-[13px] leading-5 text-app-muted">{hint}</span>
          ) : null}
        </span>
        <span className="shrink-0 pt-0.5 text-[13px] font-semibold tabular-nums text-app-subtle">
          {value > 0 ? "+" : ""}
          {value.toFixed(1)}
        </span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={0.5}
        value={value}
        onPointerDown={() => onHold(true)}
        onPointerUp={() => onHold(false)}
        onPointerCancel={() => onHold(false)}
        onChange={(event) => onChange(Number(event.target.value))}
        className="mt-2 w-full"
      />
    </label>
  );
}
