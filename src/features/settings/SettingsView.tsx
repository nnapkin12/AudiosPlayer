import { useEffect, useState } from "react";
import { Plus, X } from "lucide-react";
import { EqPanel } from "@/features/settings/EqPanel";
import { RemotePanel } from "@/features/settings/RemotePanel";
import { ThemeBuilder } from "@/features/settings/ThemeBuilder";
import { api, openExternal } from "@/lib/api";
import { errorMessage } from "@/lib/format";
import { GITHUB_URL, SVG_REPO_MARKS } from "@/lib/links";
import { ACCENTS, THEMES, newCustomTheme, readThemeColors, type CustomTheme } from "@/lib/theme";
import { useAppStore } from "@/store/useAppStore";
import { confirm } from "@/ui/confirm";

const SECTIONS = [
  ["playback", "Playback"],
  ["equalizer", "Equalizer"],
  ["appearance", "Appearance"],
  ["library", "Library"],
  ["remote", "Remote"],
  ["about", "About"],
] as const;

type Section = (typeof SECTIONS)[number][0];

export function SettingsView() {
  const snapshot = useAppStore((state) => state.snapshot);
  const libraryRoots = useAppStore((state) => state.libraryRoots);
  const theme = useAppStore((state) => state.theme);
  const accent = useAppStore((state) => state.accent);
  const customThemes = useAppStore((state) => state.customThemes);
  const minimizeMovement = useAppStore((state) => state.minimizeMovement);
  const visualizer = useAppStore((state) => state.visualizer);
  const vizMain = useAppStore((state) => state.vizMain);
  const vizBorder = useAppStore((state) => state.vizBorder);
  const vizGlow = useAppStore((state) => state.vizGlow);
  const setAppearance = useAppStore((state) => state.setAppearance);
  const setMinimizeMovement = useAppStore((state) => state.setMinimizeMovement);
  const setVisualizer = useAppStore((state) => state.setVisualizer);
  const setStatus = useAppStore((state) => state.setStatus);
  const [builder, setBuilder] = useState<CustomTheme | "new" | null>(null);
  const [section, setSection] = useState<Section>("playback");

  function applySaved(next: { theme: string; accent: string; customThemes: CustomTheme[] }) {
    setAppearance(next.theme, next.accent, next.customThemes);
  }

  async function saveAppearance(nextTheme: string, nextAccent: string) {
    setAppearance(nextTheme, nextAccent);
    try {
      applySaved(await api.setAppearance(nextTheme, nextAccent));
    } catch (error) {
      setStatus(errorMessage(error, "Could not save theme"));
    }
  }

  async function saveCustomTheme(
    draft: { name: string; colors: CustomTheme["colors"] },
    existing?: CustomTheme,
  ) {
    const theme = existing
      ? { ...existing, name: draft.name, colors: draft.colors }
      : newCustomTheme(draft.colors, draft.name);
    try {
      applySaved(await api.saveCustomTheme(theme));
      setBuilder(null);
    } catch (error) {
      setStatus(errorMessage(error, "Could not save theme"));
    }
  }

  async function removeCustomTheme(id: string) {
    const theme = customThemes.find((item) => item.id === id);
    const ok = await confirm({
      title: `Remove theme “${theme?.name ?? "Custom"}”?`,
      body: "The saved colors are gone. Built-in themes are not affected.",
      confirmLabel: "Remove theme",
      danger: true,
    });
    if (!ok) return;
    try {
      applySaved(await api.deleteCustomTheme(id));
    } catch (error) {
      setStatus(errorMessage(error, "Could not remove theme"));
    }
  }

  function saveVisualizer(next: { enabled: boolean; main: string; border: string; glow: string }) {
    const previous = { enabled: visualizer, main: vizMain, border: vizBorder, glow: vizGlow };
    setVisualizer(next);
    void api.setVisualizer(next.enabled, next.main, next.border, next.glow).catch((error) => {
      setVisualizer(previous);
      setStatus(errorMessage(error, "Could not save setting"));
    });
  }

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 gap-1 overflow-x-auto border-b border-app-line px-4">
        {SECTIONS.map(([id, label]) => (
          <button
            key={id}
            type="button"
            onClick={() => setSection(id)}
            className={`shrink-0 border-b-2 px-3 py-2.5 text-[14px] font-semibold ${
              section === id
                ? "border-app-accent text-app-text"
                : "border-transparent text-app-muted hover:text-app-text"
            }`}
          >
            {label}
          </button>
        ))}
      </div>

      <div className="page-scroll">
        <div className="mx-auto flex w-full max-w-2xl flex-col gap-5">
          {section === "playback" ? (
            <>
              <Toggle
                label="ReplayGain"
                hint="Use the volume tags on the file when they exist."
                checked={snapshot?.replaygain ?? true}
                onChange={(checked) => void api.setReplaygain(checked)}
              />
              <Toggle
                label="Gapless playback"
                hint="Start the next song as this one ends."
                checked={snapshot?.gapless ?? true}
                onChange={(checked) => void api.setGapless(checked)}
              />
            </>
          ) : null}

          {section === "equalizer" ? <EqPanel /> : null}

          {section === "appearance" ? (
            <>
              <div>
                <h2 className="mb-2 text-[13px] font-semibold text-app-muted">Theme</h2>
                <div className="flex flex-wrap gap-2">
                  {THEMES.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => void saveAppearance(item.id, accent)}
                      className={chip(theme === item.id)}
                    >
                      {item.label}
                    </button>
                  ))}
                  {customThemes.map((item) => (
                    <span key={item.id} className="inline-flex items-center">
                      <button
                        type="button"
                        onClick={() => void saveAppearance(item.id, accent)}
                        onDoubleClick={() => setBuilder(item)}
                        className={chip(theme === item.id)}
                      >
                        {item.name}
                      </button>
                      <button
                        type="button"
                        title="Remove theme"
                        aria-label={`Remove ${item.name}`}
                        onClick={() => void removeCustomTheme(item.id)}
                        className="-ml-1 rounded p-1 text-app-muted hover:text-app-text"
                      >
                        <X size={14} />
                      </button>
                    </span>
                  ))}
                  <button
                    type="button"
                    title="Create theme"
                    aria-label="Create theme"
                    onClick={() => setBuilder("new")}
                    className="flex h-8 w-8 items-center justify-center rounded-md border border-dashed border-app-border text-app-muted hover:border-app-accent hover:text-app-text"
                  >
                    <Plus size={16} />
                  </button>
                </div>
              </div>
              <div>
                <h2 className="mb-2 text-[13px] font-semibold text-app-muted">Accent</h2>
                <div className="flex flex-wrap gap-2">
                  {ACCENTS.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => void saveAppearance(theme, item.id)}
                      className={chip(accent === item.id)}
                    >
                      {item.label}
                    </button>
                  ))}
                </div>
              </div>
              <Toggle
                label="Minimize movement"
                hint="Fewer animations."
                checked={minimizeMovement}
                onChange={(checked) => {
                  setMinimizeMovement(checked);
                  void api.setMinimizeMovement(checked).catch((error) => {
                    setMinimizeMovement(!checked);
                    setStatus(errorMessage(error, "Could not save setting"));
                  });
                }}
              />
              <Toggle
                label="Audio Visualizer"
                hint="Bars in the now-playing bar. Off uses no extra CPU."
                checked={visualizer}
                onChange={(checked) => {
                  saveVisualizer({
                    enabled: checked,
                    main: vizMain,
                    border: vizBorder,
                    glow: vizGlow,
                  });
                }}
              />
              <VizColors
                main={vizMain}
                border={vizBorder}
                glow={vizGlow}
                onChange={(colors) => {
                  saveVisualizer({ enabled: visualizer, ...colors });
                }}
              />
            </>
          ) : null}

          {section === "library" ? (
            libraryRoots.length === 0 ? (
              <p className="text-[15px] text-app-muted">No folders yet.</p>
            ) : (
              <ul className="flex flex-col gap-2">
                {libraryRoots.map((path) => (
                  <li key={path} className="break-all text-[15px] text-app-subtle">
                    {path}
                  </li>
                ))}
              </ul>
            )
          ) : null}

          {section === "remote" ? <RemotePanel /> : null}

          {section === "about" ? (
            <div className="flex flex-col items-start gap-2">
              <p className="text-[14px] text-app-muted">The Audios! logo uses these SVGs.</p>
              {SVG_REPO_MARKS.map((item) => (
                <button
                  key={item.href}
                  type="button"
                  onClick={() => void openExternal(item.href)}
                  className="text-[15px] font-semibold text-app-accent hover:underline"
                >
                  {item.name}
                </button>
              ))}
              <button
                type="button"
                onClick={() => void openExternal(GITHUB_URL)}
                className="text-[15px] font-semibold text-app-accent hover:underline"
              >
                GitHub
              </button>
            </div>
          ) : null}
        </div>
      </div>
      {builder ? (
        <ThemeBuilder
          initialName={builder === "new" ? "Custom theme" : builder.name}
          initialColors={builder === "new" ? readThemeColors() : builder.colors}
          onCancel={() => setBuilder(null)}
          onSave={(draft) => void saveCustomTheme(draft, builder === "new" ? undefined : builder)}
        />
      ) : null}
    </section>
  );
}

function chip(active: boolean): string {
  return `rounded-md border px-3 py-1.5 text-[14px] font-semibold ${
    active
      ? "border-app-accent bg-app-hover text-app-text"
      : "border-app-border text-app-subtle hover:bg-app-hover"
  }`;
}

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 border-b border-app-line py-3">
      <span>
        <span className="block text-[15px] font-semibold text-app-text">{label}</span>
        <span className="mt-0.5 block text-[13px] text-app-muted">{hint}</span>
      </span>
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="h-4 w-4"
      />
    </label>
  );
}

const HEX = /^#[0-9a-fA-F]{6}$/;

function VizColors({
  main,
  border,
  glow,
  onChange,
}: {
  main: string;
  border: string;
  glow: string;
  onChange: (colors: { main: string; border: string; glow: string }) => void;
}) {
  return (
    <div className="flex flex-col gap-2 py-3">
      <VizColor
        label="Main Color"
        value={main}
        onChange={(value) => onChange({ main: value, border, glow })}
      />
      <VizColor
        label="Border Color"
        value={border}
        onChange={(value) => onChange({ main, border: value, glow })}
      />
      <VizColor
        label="Glow Color"
        value={glow}
        onChange={(value) => onChange({ main, border, glow: value })}
      />
    </div>
  );
}

function VizColor({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => {
    setDraft(value);
  }, [value]);
  return (
    <label className="flex items-center justify-between gap-3">
      <span className="text-[14px] font-semibold text-app-subtle">{label}</span>
      <span className="flex items-center gap-2">
        <input
          type="color"
          value={HEX.test(value) ? value : "#8ec8ff"}
          onChange={(event) => onChange(event.target.value)}
          className="h-8 w-10 cursor-pointer rounded border border-app-border bg-transparent p-0"
          aria-label={label}
        />
        <input
          value={draft}
          onChange={(event) => {
            const next = event.target.value;
            setDraft(next);
            if (HEX.test(next)) onChange(next);
          }}
          onBlur={() => {
            if (!HEX.test(draft)) setDraft(value);
          }}
          spellCheck={false}
          className="w-[92px] rounded-md border border-app-border bg-app px-2 py-1 font-mono text-[13px] text-app-text"
        />
      </span>
    </label>
  );
}
