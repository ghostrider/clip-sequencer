// Colour themes (SPEC §9.7). A theme is a full set of colour tokens plus a "look": fonts, corner
// radii and label style. The five built-in themes each have their own look; a theme the user saves
// keeps the look of the theme it was made from (`base`) and stores only its colours.
// `web/src/tokens.css` holds the default theme (Rack) so the page is styled before this runs.

export const DEFAULT_THEME_ID = 'rack';

/** Every colour token, grouped as the Preferences colour editor shows them. */
export const COLOR_GROUPS = [
  {
    label: 'Surfaces',
    tokens: [
      ['bg', 'Background'],
      ['panel', 'Panels'],
      ['surface', 'Cards, dialogs'],
      ['track', 'Lane grid'],
      ['control', 'Buttons, inputs'],
      ['scrim', 'Dialog backdrop'],
    ],
  },
  {
    label: 'Lines',
    tokens: [
      ['line', 'Separators'],
      ['line-soft', 'Lane separators'],
      ['line-strong', 'Control borders'],
      ['step-line', 'Step borders'],
      ['tick', 'Ruler ticks'],
      ['tick-strong', 'Ruler beats'],
    ],
  },
  {
    label: 'Text',
    tokens: [
      ['text', 'Text'],
      ['text-2', 'Secondary text'],
      ['muted', 'Labels'],
      ['faint', 'Hints, disabled'],
    ],
  },
  {
    label: 'Accent: running, selected, playhead',
    tokens: [
      ['accent', 'Accent'],
      ['accent-ink', 'Text on accent'],
      ['accent-bg', 'Accent background'],
      ['accent-line', 'Accent border'],
      ['accent-text', 'Accent text'],
    ],
  },
  {
    label: 'Info: confirmed by Resolume, bindings',
    tokens: [
      ['info-text', 'Info text'],
      ['info-bg', 'Info background'],
      ['info-line', 'Info border'],
    ],
  },
  {
    label: 'OK: connections',
    tokens: [
      ['ok', 'OK'],
      ['ok-ink', 'Text on OK'],
      ['ok-bg', 'OK background'],
      ['ok-line', 'OK border'],
      ['ok-text', 'OK text'],
    ],
  },
  {
    label: 'Error: unresolved, yielded, disconnected',
    tokens: [
      ['err', 'Error'],
      ['err-ink', 'Text on error'],
      ['err-bg', 'Error background'],
      ['err-line', 'Error border'],
      ['err-text', 'Error text'],
    ],
  },
  {
    label: 'Transitions: the fade of a step',
    tokens: [['fade', 'Fade']],
  },
] as const;

export type ColorToken = (typeof COLOR_GROUPS)[number]['tokens'][number][0];
export const COLOR_TOKENS: ColorToken[] = COLOR_GROUPS.flatMap((g) => g.tokens.map((t) => t[0]));
export type Colors = Record<ColorToken, string>;

/** Bundled font families (all SIL OFL 1.1, see scripts/third-party-licenses.mjs); a theme picks one per role. */
export const FONTS = [
  { id: 'barlow', name: 'Barlow', stack: "'Barlow', system-ui, sans-serif", mono: false },
  { id: 'barlow-condensed', name: 'Barlow Condensed', stack: "'Barlow Condensed', 'Arial Narrow', sans-serif", mono: false },
  { id: 'manrope', name: 'Manrope', stack: "'Manrope', system-ui, sans-serif", mono: false },
  { id: 'chakra-petch', name: 'Chakra Petch', stack: "'Chakra Petch', system-ui, sans-serif", mono: false },
  { id: 'archivo', name: 'Archivo', stack: "'Archivo', system-ui, sans-serif", mono: false },
  { id: 'archivo-narrow', name: 'Archivo Narrow', stack: "'Archivo Narrow', 'Arial Narrow', sans-serif", mono: false },
  { id: 'ibm-plex-sans', name: 'IBM Plex Sans', stack: "'IBM Plex Sans', system-ui, sans-serif", mono: false },
  { id: 'jetbrains-mono', name: 'JetBrains Mono', stack: "'JetBrains Mono', ui-monospace, monospace", mono: true },
  { id: 'fragment-mono', name: 'Fragment Mono', stack: "'Fragment Mono', ui-monospace, monospace", mono: true },
  { id: 'dm-mono', name: 'DM Mono', stack: "'DM Mono', ui-monospace, monospace", mono: true },
  { id: 'ibm-plex-mono', name: 'IBM Plex Mono', stack: "'IBM Plex Mono', ui-monospace, monospace", mono: true },
] as const;
export type FontId = (typeof FONTS)[number]['id'];

/** The font of each text role. */
export const FONT_ROLES = [
  ['ui', 'Text'],
  ['label', 'Labels (uppercase)'],
  ['head', 'Sequence title'],
  ['mono', 'Numbers, names, positions'],
] as const;
export type FontRole = (typeof FONT_ROLES)[number][0];
export type ThemeFonts = Record<FontRole, FontId>;

function fontStack(id: string | undefined, role: FontRole): string {
  const f = FONTS.find((x) => x.id === id) ?? FONTS.find((x) => x.id === (role === 'mono' ? 'jetbrains-mono' : 'barlow'))!;
  return f.stack;
}

/** Corners and label style of a built-in theme; not editable (a saved theme keeps its base's). */
export interface Look {
  headCase: 'none' | 'uppercase';
  labelSpacing: string;
  radius: string;
  radiusSm: string;
  radiusLg: string;
  radiusPill: string;
}

export interface Theme {
  id: string;
  name: string;
  builtin: boolean;
  /** The built-in theme whose look this one uses (itself for a built-in one). */
  base: string;
  colors: Colors;
  fonts: ThemeFonts;
}

/** A theme the user saved (`settings.json`, `custom_themes`). */
export interface CustomTheme {
  id: string;
  name: string;
  base: string;
  colors: Partial<Colors>;
  fonts?: Partial<ThemeFonts>;
}

const rackLook: Look = { headCase: 'uppercase', labelSpacing: '.12em', radius: '1px', radiusSm: '1px', radiusLg: '2px', radiusPill: '1px' };

export const LOOKS: Record<string, Look> = {
  rack: rackLook,
  ink: { headCase: 'none', labelSpacing: '.08em', radius: '3px', radiusSm: '2px', radiusLg: '4px', radiusPill: '3px' },
  graphite: { headCase: 'none', labelSpacing: '.1em', radius: '0', radiusSm: '0', radiusLg: '0', radiusPill: '0' },
  classic: { headCase: 'none', labelSpacing: '.1em', radius: '6px', radiusSm: '4px', radiusLg: '8px', radiusPill: '999px' },
  daylight: rackLook,
};

const rackFonts: ThemeFonts = { ui: 'barlow', label: 'barlow-condensed', head: 'barlow-condensed', mono: 'jetbrains-mono' };

export const BUILTIN_THEMES: Theme[] = [
  {
    id: 'rack', name: 'Rack', builtin: true, base: 'rack', fonts: rackFonts,
    colors: {
      bg: '#14181D', panel: '#1B2027', surface: '#222831', track: '#171B21', control: '#29303A', scrim: '#05070ACC',
      line: '#0E1115', 'line-soft': '#20262D', 'line-strong': '#3A4450', 'step-line': '#3A4450', tick: '#252C34', 'tick-strong': '#38424D',
      text: '#EEF2F5', 'text-2': '#C3CCD5', muted: '#8C98A5', faint: '#6E7A86',
      accent: '#2FD8F0', 'accent-ink': '#021A1F', 'accent-bg': '#0F2C34', 'accent-line': '#1F6273', 'accent-text': '#A6F2FF',
      'info-text': '#B9BCFF', 'info-bg': '#1B1C33', 'info-line': '#3D4080',
      ok: '#6BD45A', 'ok-ink': '#0A1A06', 'ok-bg': '#16241A', 'ok-line': '#2E5A2A', 'ok-text': '#C2EDB8',
      err: '#FF5A4E', 'err-ink': '#FFFFFF', 'err-bg': '#2A1517', 'err-line': '#7E2F2A', 'err-text': '#FFB8B0',
      fade: '#B08CFF',
    },
  },
  {
    id: 'ink', name: 'Ink & Signal', builtin: true, base: 'ink',
    fonts: { ui: 'manrope', label: 'chakra-petch', head: 'chakra-petch', mono: 'fragment-mono' },
    colors: {
      bg: '#0B0D1E', panel: '#11142C', surface: '#181C3B', track: '#0E1126', control: '#1F2447', scrim: '#04051099',
      line: '#262B55', 'line-soft': '#1C2044', 'line-strong': '#383F75', 'step-line': '#383F75', tick: '#1E2246', 'tick-strong': '#323866',
      text: '#EEEDFF', 'text-2': '#C6C5EC', muted: '#8E8FBD', faint: '#6D6EA0',
      accent: '#FF3D9A', 'accent-ink': '#24001A', 'accent-bg': '#35103A', 'accent-line': '#7A2369', 'accent-text': '#FFB6DB',
      'info-text': '#7FE8FF', 'info-bg': '#0D2236', 'info-line': '#1D5573',
      ok: '#47DE9C', 'ok-ink': '#04170F', 'ok-bg': '#0F2526', 'ok-line': '#1F5A4B', 'ok-text': '#B4F2D6',
      err: '#FF6A3D', 'err-ink': '#1F0800', 'err-bg': '#2A1424', 'err-line': '#80361F', 'err-text': '#FFC0A8',
      fade: '#A99BFF',
    },
  },
  {
    id: 'graphite', name: 'Graphite & Lime', builtin: true, base: 'graphite',
    fonts: { ui: 'archivo', label: 'archivo-narrow', head: 'archivo', mono: 'dm-mono' },
    colors: {
      bg: '#2A2C2F', panel: '#323438', surface: '#3A3D41', track: '#2E3033', control: '#43464B', scrim: '#000000AA',
      line: '#222427', 'line-soft': '#28292C', 'line-strong': '#53575D', 'step-line': '#53575D', tick: '#3A3D41', 'tick-strong': '#4E5157',
      text: '#F4F5F1', 'text-2': '#D4D6D0', muted: '#A4A8A1', faint: '#868A84',
      accent: '#C6F432', 'accent-ink': '#1A2200', 'accent-bg': '#3E4722', 'accent-line': '#6E8A1C', 'accent-text': '#E3FF8F',
      'info-text': '#A9C9FF', 'info-bg': '#2C3445', 'info-line': '#4A5E86',
      ok: '#30C7B1', 'ok-ink': '#03201B', 'ok-bg': '#283B39', 'ok-line': '#2F6B62', 'ok-text': '#B4EEE4',
      err: '#FF6262', 'err-ink': '#2A0000', 'err-bg': '#4A2C2E', 'err-line': '#9A3E40', 'err-text': '#FFC4C4',
      fade: '#C2A6FF',
    },
  },
  {
    id: 'classic', name: 'Classic Amber', builtin: true, base: 'classic',
    fonts: { ui: 'ibm-plex-sans', label: 'ibm-plex-sans', head: 'ibm-plex-sans', mono: 'ibm-plex-mono' },
    colors: {
      bg: '#0E0F11', panel: '#131417', surface: '#16181B', track: '#121316', control: '#1C1E22', scrim: '#00000099',
      line: '#26292E', 'line-soft': '#1F2125', 'line-strong': '#33373D', 'step-line': '#3A3D44', tick: '#1E2024', 'tick-strong': '#2C2F35',
      text: '#E6E6E3', 'text-2': '#C9CCD1', muted: '#9A9EA6', faint: '#7E838B',
      accent: '#F2A33A', 'accent-ink': '#1A1206', 'accent-bg': '#2A1E0E', 'accent-line': '#5A4220', 'accent-text': '#FFD39A',
      'info-text': '#9DCBF0', 'info-bg': '#121C26', 'info-line': '#2A4A66',
      ok: '#4CC26E', 'ok-ink': '#0B1A10', 'ok-bg': '#13221A', 'ok-line': '#2F5B3A', 'ok-text': '#B6E3C2',
      err: '#E5484D', 'err-ink': '#FFFFFF', 'err-bg': '#1E1214', 'err-line': '#7A2A2F', 'err-text': '#FFB4B7',
      fade: '#B79AF0',
    },
  },
  {
    // For bright rooms (daytime rehearsals, outdoor stages): dark text on light panels.
    id: 'daylight', name: 'Daylight', builtin: true, base: 'daylight', fonts: rackFonts,
    colors: {
      bg: '#E9ECEF', panel: '#F5F6F8', surface: '#FFFFFF', track: '#DDE1E6', control: '#FFFFFF', scrim: '#1B202766',
      line: '#C3C9D0', 'line-soft': '#CFD4DA', 'line-strong': '#9AA3AD', 'step-line': '#8C96A1', tick: '#C3C9D0', 'tick-strong': '#9AA3AD',
      text: '#111418', 'text-2': '#2C333B', muted: '#56606B', faint: '#6F7983',
      accent: '#0072D6', 'accent-ink': '#FFFFFF', 'accent-bg': '#D6E8FA', 'accent-line': '#6FA8E0', 'accent-text': '#004A8F',
      'info-text': '#5A3FB8', 'info-bg': '#ECE7FB', 'info-line': '#A996E6',
      ok: '#1E8A3A', 'ok-ink': '#FFFFFF', 'ok-bg': '#E0F2E4', 'ok-line': '#8CC79A', 'ok-text': '#145C27',
      err: '#D11F2A', 'err-ink': '#FFFFFF', 'err-bg': '#FBE3E4', 'err-line': '#E79A9E', 'err-text': '#9A121B',
      fade: '#7442C8',
    },
  },
];

export function builtinTheme(id: string): Theme | undefined {
  return BUILTIN_THEMES.find((t) => t.id === id);
}

/** A saved theme as a full theme: unknown base → the default look, missing colours and fonts → its base's. */
export function fromCustom(c: CustomTheme): Theme {
  const base = builtinTheme(c.base) ?? builtinTheme(DEFAULT_THEME_ID)!;
  const colors = { ...base.colors };
  for (const t of COLOR_TOKENS) {
    const v = c.colors[t];
    if (v && HEX.test(v)) colors[t] = v.toUpperCase();
  }
  const fonts = { ...base.fonts };
  for (const [role] of FONT_ROLES) {
    const f = c.fonts?.[role];
    if (f && FONTS.some((x) => x.id === f)) fonts[role] = f;
  }
  return { id: c.id, name: c.name, builtin: false, base: base.id, colors, fonts };
}

/** The selected theme among the built-in and saved ones; the default if it no longer exists. */
export function resolveTheme(id: string | null | undefined, custom: CustomTheme[]): Theme {
  const b = id ? builtinTheme(id) : undefined;
  if (b) return b;
  const c = id ? custom.find((t) => t.id === id) : undefined;
  return c ? fromCustom(c) : builtinTheme(DEFAULT_THEME_ID)!;
}

export const HEX = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;

/** Relative luminance of `#RRGGBB` (0 = black, 1 = white). */
function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}

const CACHE_KEY = 'seq.theme';

/** Sets the theme's tokens on the root element (they override tokens.css). */
export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  const look = LOOKS[theme.base] ?? LOOKS[DEFAULT_THEME_ID];
  for (const t of COLOR_TOKENS) root.style.setProperty(`--${t}`, theme.colors[t]);
  const vars: Record<string, string> = {
    font: fontStack(theme.fonts?.ui, 'ui'), mono: fontStack(theme.fonts?.mono, 'mono'),
    'font-label': fontStack(theme.fonts?.label, 'label'), 'font-head': fontStack(theme.fonts?.head, 'head'), 'head-case': look.headCase,
    'label-ls': look.labelSpacing, radius: look.radius, 'radius-sm': look.radiusSm, 'radius-lg': look.radiusLg,
    'radius-pill': look.radiusPill,
  };
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(`--${k}`, v);
  // Native scrollbars and form controls follow the background.
  root.style.colorScheme = luminance(theme.colors.bg) > 0.4 ? 'light' : 'dark';
  root.dataset.theme = theme.id;
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(theme));
  } catch {
    /* private mode: the default shows until the app sends the saved theme */
  }
}

/** The theme shown last in this browser, applied before the app connects (no flash of the default). */
export function applyCachedTheme() {
  try {
    const raw = localStorage.getItem(CACHE_KEY);
    if (!raw) return;
    const t = JSON.parse(raw) as Theme;
    if (t && typeof t.base === 'string' && t.colors && COLOR_TOKENS.every((k) => HEX.test(t.colors[k] ?? ''))) {
      applyTheme(t);
    }
  } catch {
    /* ignore a broken cache entry */
  }
}

/** A new id for a saved theme. */
export function newThemeId(): string {
  const r = crypto.getRandomValues(new Uint8Array(6));
  return 'custom-' + Array.from(r, (b) => b.toString(16).padStart(2, '0')).join('');
}
