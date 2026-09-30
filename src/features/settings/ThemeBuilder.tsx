import { useEffect, useRef, useState } from "react";
import { useFocusTrap } from "@/ui/useFocusTrap";
import type { SimpleThemeKey, ThemeColors } from "@/lib/theme";
import {
  SIMPLE_THEME_FIELDS,
  THEME_COLOR_FIELDS,
  applySimpleThemeColor,
  cloneThemeColors,
  normalizeHex,
} from "@/lib/theme";

export function ThemeBuilder({
  initialName,
  initialColors,
  onCancel,
  onSave,
}: {
  initialName: string;
  initialColors: ThemeColors;
  onCancel: () => void;
  onSave: (theme: { name: string; colors: ThemeColors }) => void;
}) {
  const [name, setName] = useState(initialName);
  const [colors, setColors] = useState(() => cloneThemeColors(initialColors));
  const [advanced, setAdvanced] = useState(false);
  const dialogRef = useRef<HTMLDivElement>(null);
  useFocusTrap(dialogRef, true);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  function update(key: keyof ThemeColors, value: string) {
    setColors((current) => ({ ...current, [key]: normalizeHex(value) }));
  }

  function updateSimple(key: SimpleThemeKey, value: string) {
    setColors((current) => applySimpleThemeColor(current, key, value));
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 px-4">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="theme-builder-title"
        className="flex max-h-[min(760px,90vh)] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-app-border bg-app-raised shadow-[0_24px_60px_rgb(0_0_0_/_0.45)]"
      >
        <div className="flex items-center justify-between border-b border-app-line px-5 py-3">
          <h2 id="theme-builder-title" className="text-[16px] font-semibold">
            Theme builder
          </h2>
          <button
            type="button"
            onClick={onCancel}
            className="text-[14px] font-semibold text-app-muted hover:text-app-text"
          >
            Close
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-auto px-5 py-4">
          <ThemePreview colors={colors} name={name.trim() || "Custom theme"} />
          <div className="mt-5 grid grid-cols-1 gap-3 sm:grid-cols-2">
            {SIMPLE_THEME_FIELDS.map((field) => (
              <ColorField
                key={field.key}
                label={field.label}
                value={colors[field.key]}
                onChange={(value) => updateSimple(field.key, value)}
              />
            ))}
          </div>
          <label className="mt-5 flex cursor-pointer items-start justify-between gap-4">
            <span>
              <span className="block text-[16px] font-semibold text-app-text">Advanced</span>
              <span className="mt-1 block text-[14px] leading-6 text-app-muted">
                Edit each color on its own.
              </span>
            </span>
            <input
              type="checkbox"
              checked={advanced}
              onChange={(event) => setAdvanced(event.target.checked)}
              className="mt-1 h-4 w-4"
            />
          </label>
          {advanced ? (
            <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              {THEME_COLOR_FIELDS.map((field) => (
                <ColorField
                  key={field.key}
                  label={field.label}
                  value={colors[field.key]}
                  onChange={(value) => update(field.key, value)}
                />
              ))}
            </div>
          ) : null}
        </div>

        <form
          className="flex flex-col gap-3 border-t border-app-line px-5 py-4 sm:flex-row sm:items-center"
          onSubmit={(event) => {
            event.preventDefault();
            onSave({
              name: name.trim() || "Custom theme",
              colors,
            });
          }}
        >
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Theme name"
            className="min-w-0 flex-1 rounded-lg border border-app-border bg-app px-3 py-2 text-[15px] text-app-text"
          />
          <div className="flex gap-2">
            <button
              type="button"
              onClick={onCancel}
              className="rounded-lg border border-app-border px-3 py-2 text-[14px] font-semibold text-app-subtle hover:bg-app-hover"
            >
              Cancel
            </button>
            <button
              type="submit"
              className="rounded-lg bg-app-play px-3 py-2 text-[14px] font-semibold text-app-play-fg"
            >
              Save theme
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function ColorField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="flex items-center justify-between gap-3">
      <span className="text-[14px] font-semibold text-app-subtle">{label}</span>
      <span className="flex items-center gap-2">
        <input
          type="color"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          className="h-8 w-10 cursor-pointer rounded border border-app-border bg-transparent p-0"
          aria-label={label}
        />
        <input
          value={value}
          onChange={(event) => onChange(event.target.value)}
          spellCheck={false}
          className="w-[92px] rounded-md border border-app-border bg-app px-2 py-1 font-mono text-[13px] text-app-text"
        />
      </span>
    </label>
  );
}

function ThemePreview({ colors, name }: { colors: ThemeColors; name: string }) {
  return (
    <div
      className="overflow-hidden rounded-lg border"
      style={{ background: colors.app, borderColor: colors.border, color: colors.text }}
    >
      <div
        className="flex items-center justify-between px-3 py-2 text-[12px] font-semibold"
        style={{ background: colors.raised, borderBottom: `1px solid ${colors.line}` }}
      >
        <span className="flex gap-1">
          <span className="h-2 w-2 rounded-full" style={{ background: colors.danger }} />
          <span className="h-2 w-2 rounded-full" style={{ background: colors.accent }} />
          <span className="h-2 w-2 rounded-full" style={{ background: colors.play }} />
        </span>
        <span style={{ color: colors.subtle }}>{name}</span>
        <span className="w-8" />
      </div>
      <div className="flex h-[118px]">
        <div
          className="flex w-10 flex-col items-center gap-2 py-3"
          style={{ background: colors.raised, borderRight: `1px solid ${colors.line}` }}
        >
          <span className="h-5 w-5 rounded-md" style={{ background: colors.hover }} />
          <span className="h-5 w-5 rounded-md" style={{ background: colors.accent }} />
        </div>
        <div className="flex min-w-0 flex-1 flex-col justify-between p-3">
          <div>
            <p className="text-[15px] font-semibold">Preview track</p>
            <p className="text-[12px] font-medium" style={{ color: colors.muted }}>
              Artist · Album
            </p>
          </div>
          <div className="flex items-center gap-2">
            <span
              className="rounded-full px-3 py-1 text-[12px] font-semibold"
              style={{ background: colors.play, color: colors.playFg }}
            >
              Play
            </span>
            <span className="h-1 flex-1 rounded-full" style={{ background: colors.border }}>
              <span
                className="block h-1 w-1/3 rounded-full"
                style={{ background: colors.accent }}
              />
            </span>
          </div>
        </div>
      </div>
      <div
        className="px-3 py-2 text-[11px] font-semibold"
        style={{
          background: colors.bar,
          borderTop: `2px solid ${colors.barLine}`,
          color: colors.subtle,
        }}
      >
        Now playing
      </div>
    </div>
  );
}
