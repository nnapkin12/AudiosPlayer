export const THEMES = [
  { id: "dusk", label: "Dusk" },
  { id: "midnight", label: "Midnight" },
  { id: "slate", label: "Slate" },
  { id: "paper", label: "Paper" },
] as const;

export const ACCENTS = [
  { id: "blue", label: "Blue" },
  { id: "amber", label: "Amber" },
  { id: "sage", label: "Sage" },
  { id: "rose", label: "Rose" },
  { id: "violet", label: "Violet" },
] as const;

export type ThemeId = (typeof THEMES)[number]["id"];
export type AccentId = (typeof ACCENTS)[number]["id"];

export interface ThemeColors {
  frame: string;
  app: string;
  raised: string;
  bar: string;
  barLine: string;
  hover: string;
  border: string;
  line: string;
  muted: string;
  text: string;
  subtle: string;
  danger: string;
  play: string;
  playFg: string;
  accent: string;
  accentDim: string;
}

export interface CustomTheme {
  id: string;
  name: string;
  colors: ThemeColors;
}

export const THEME_COLOR_FIELDS: Array<{
  key: keyof ThemeColors;
  label: string;
  css: string;
}> = [
  { key: "frame", label: "Frame", css: "--app-frame" },
  { key: "app", label: "Background", css: "--app" },
  { key: "raised", label: "Raised", css: "--app-raised" },
  { key: "bar", label: "Bar", css: "--app-bar" },
  { key: "barLine", label: "Bar line", css: "--app-bar-line" },
  { key: "hover", label: "Hover", css: "--app-hover" },
  { key: "border", label: "Border", css: "--app-border" },
  { key: "line", label: "Line", css: "--app-line" },
  { key: "muted", label: "Muted text", css: "--app-muted" },
  { key: "text", label: "Text", css: "--app-text" },
  { key: "subtle", label: "Subtle text", css: "--app-subtle" },
  { key: "danger", label: "Danger", css: "--app-danger" },
  { key: "play", label: "Play button", css: "--app-play" },
  { key: "playFg", label: "Play icon", css: "--app-play-fg" },
  { key: "accent", label: "Accent", css: "--app-accent" },
  { key: "accentDim", label: "Accent dim", css: "--app-accent-dim" },
];

export const SIMPLE_THEME_FIELDS = [
  { key: "app", label: "Background" },
  { key: "text", label: "Text" },
  { key: "accent", label: "Accent" },
  { key: "play", label: "Play button" },
  { key: "danger", label: "Danger" },
] as const;

export type SimpleThemeKey = (typeof SIMPLE_THEME_FIELDS)[number]["key"];

export const DEFAULT_THEME_COLORS: ThemeColors = {
  frame: "#141414",
  app: "#1e1e1e",
  raised: "#252526",
  bar: "#0e0e0e",
  barLine: "#606060",
  hover: "#2d2d2d",
  border: "#3c3c3c",
  line: "#333333",
  muted: "#9a9a9a",
  text: "#ececec",
  subtle: "#d2d2d2",
  danger: "#c97a7a",
  play: "#efefef",
  playFg: "#1e1e1e",
  accent: "#7b9ecf",
  accentDim: "#4d6688",
};

const ACCENT_COLORS: Record<string, Pick<ThemeColors, "accent" | "accentDim">> = {
  blue: { accent: "#7b9ecf", accentDim: "#4d6688" },
  amber: { accent: "#d6a858", accentDim: "#967030" },
  sage: { accent: "#84a87e", accentDim: "#4e704a" },
  rose: { accent: "#cc7e8a", accentDim: "#8c4e58" },
  violet: { accent: "#a48acc", accentDim: "#6c5694" },
};

export function normalizeHex(value: string): string {
  const raw = value.trim();
  const short = /^#?([0-9a-f]{3})$/i.exec(raw);
  if (short) {
    const [r, g, b] = short[1].split("");
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  const full = /^#?([0-9a-f]{6})$/i.exec(raw);
  if (!full) return "#000000";
  return `#${full[1].toLowerCase()}`;
}

export function rgbToHex(value: string): string {
  const trimmed = value.trim();
  if (trimmed.startsWith("#")) return normalizeHex(trimmed);
  const rgb = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(trimmed);
  if (rgb) {
    return normalizeHex(
      `#${[rgb[1], rgb[2], rgb[3]]
        .map((part) => Number(part).toString(16).padStart(2, "0"))
        .join("")}`,
    );
  }
  const parts = trimmed
    .split(/[,\s/]+/)
    .filter(Boolean)
    .map(Number);
  if (parts.length < 3 || parts.some((part) => !Number.isFinite(part))) {
    return "#000000";
  }
  return normalizeHex(
    `#${parts
      .slice(0, 3)
      .map((part) =>
        Math.max(0, Math.min(255, Math.round(part)))
          .toString(16)
          .padStart(2, "0"),
      )
      .join("")}`,
  );
}

function hexChannels(hex: string): [number, number, number] {
  const n = Number.parseInt(normalizeHex(hex).slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function hexToRgb(hex: string): string {
  return hexChannels(hex).join(" ");
}

export function mixHex(a: string, b: string, amount: number): string {
  const t = Math.min(1, Math.max(0, amount));
  const [ar, ag, ab] = hexChannels(a);
  const [br, bg, bb] = hexChannels(b);
  const channel = (from: number, to: number) => Math.round(from + (to - from) * t);
  return normalizeHex(
    `#${[channel(ar, br), channel(ag, bg), channel(ab, bb)]
      .map((n) => n.toString(16).padStart(2, "0"))
      .join("")}`,
  );
}

function isLight(hex: string): boolean {
  const [r, g, b] = hexChannels(hex);
  return (r * 299 + g * 587 + b * 114) / 1000 > 140;
}

export function applySimpleThemeColor(
  colors: ThemeColors,
  key: SimpleThemeKey,
  value: string,
): ThemeColors {
  const hex = normalizeHex(value);
  const next = cloneThemeColors(colors);
  if (key === "app") {
    const light = isLight(hex);
    const ink = light ? "#000000" : "#ffffff";
    next.app = hex;
    next.frame = mixHex(hex, "#000000", light ? 0.12 : 0.22);
    next.raised = mixHex(hex, "#ffffff", light ? 0.16 : 0.05);
    next.bar = mixHex(hex, "#000000", light ? 0.1 : 0.36);
    next.barLine = mixHex(hex, ink, light ? 0.32 : 0.28);
    next.hover = mixHex(hex, ink, 0.08);
    next.border = mixHex(hex, ink, light ? 0.16 : 0.14);
    next.line = mixHex(hex, ink, 0.1);
  } else if (key === "text") {
    next.text = hex;
    next.subtle = mixHex(hex, next.app, 0.18);
    next.muted = mixHex(hex, next.app, 0.42);
  } else if (key === "accent") {
    next.accent = hex;
    next.accentDim = mixHex(hex, "#000000", 0.38);
  } else if (key === "play") {
    next.play = hex;
    next.playFg = isLight(hex) ? "#141414" : "#f2f2f2";
  } else {
    next.danger = hex;
  }
  return next;
}

export function cloneThemeColors(colors: ThemeColors): ThemeColors {
  return { ...colors };
}

export function applyThemeColors(colors: ThemeColors) {
  const root = document.documentElement;
  for (const field of THEME_COLOR_FIELDS) {
    root.style.setProperty(field.css, hexToRgb(colors[field.key]));
  }
}

export function clearThemeColors() {
  const root = document.documentElement;
  for (const field of THEME_COLOR_FIELDS) {
    root.style.removeProperty(field.css);
  }
}

export function readThemeColors(): ThemeColors {
  if (typeof document === "undefined") return cloneThemeColors(DEFAULT_THEME_COLORS);
  const style = getComputedStyle(document.documentElement);
  const next = cloneThemeColors(DEFAULT_THEME_COLORS);
  for (const field of THEME_COLOR_FIELDS) {
    const value = style.getPropertyValue(field.css);
    if (value.trim()) next[field.key] = rgbToHex(value);
  }
  return next;
}

export function colorsForBuiltin(theme: string, accent: string): ThemeColors {
  const colors = cloneThemeColors(DEFAULT_THEME_COLORS);
  if (theme === "midnight") {
    colors.frame = "#08080a";
    colors.app = "#101012";
    colors.raised = "#16161a";
    colors.bar = "#040406";
    colors.barLine = "#4e4e58";
    colors.hover = "#202026";
    colors.border = "#34343c";
    colors.line = "#28282e";
    colors.muted = "#94949c";
    colors.text = "#f4f4f8";
    colors.subtle = "#d6d6de";
    colors.danger = "#d27880";
    colors.play = "#f4f4f8";
    colors.playFg = "#101012";
  } else if (theme === "slate") {
    colors.frame = "#12161a";
    colors.app = "#1c2228";
    colors.raised = "#242c34";
    colors.bar = "#0c1014";
    colors.barLine = "#607080";
    colors.hover = "#2e3842";
    colors.border = "#44505c";
    colors.line = "#38424c";
    colors.muted = "#9ca8b2";
    colors.text = "#ecf2f6";
    colors.subtle = "#d2dce4";
    colors.danger = "#d28282";
    colors.play = "#ecf2f6";
    colors.playFg = "#1c2228";
  } else if (theme === "paper") {
    colors.frame = "#d6d0c4";
    colors.app = "#f4efe6";
    colors.raised = "#faf6ee";
    colors.bar = "#dcd2c0";
    colors.barLine = "#94846e";
    colors.hover = "#e8e0d2";
    colors.border = "#c6bcac";
    colors.line = "#d2c8b8";
    colors.muted = "#6e665a";
    colors.text = "#24201c";
    colors.subtle = "#403a32";
    colors.danger = "#a84840";
    colors.play = "#24201c";
    colors.playFg = "#f4efe6";
  }
  const accentColors = ACCENT_COLORS[accent] ?? ACCENT_COLORS.blue;
  colors.accent = accentColors.accent;
  colors.accentDim = accentColors.accentDim;
  return colors;
}

export function applyAppearance(theme: string, accent: string, customThemes: CustomTheme[] = []) {
  const root = document.documentElement;
  const custom = customThemes.find((item) => item.id === theme);
  if (custom) {
    root.dataset.theme = "custom";
    applyThemeColors(custom.colors);
  } else {
    clearThemeColors();
    root.dataset.theme = THEMES.some((item) => item.id === theme) ? theme : "dusk";
  }
  root.dataset.accent = ACCENTS.some((item) => item.id === accent) ? accent : "blue";
}

export function newCustomTheme(colors: ThemeColors, name = "Custom theme"): CustomTheme {
  const id =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? `custom-${crypto.randomUUID()}`
      : `custom-${Date.now()}`;
  return { id, name, colors: cloneThemeColors(colors) };
}
