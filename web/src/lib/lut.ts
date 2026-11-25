// LUT tab helpers (SPEC §14): defaults, the curve spline for drawing, the bundled test images and the WebGL2
// preview. The colour math itself lives in crates/lut-core; the preview applies the table the app baked.
import type { Curve, Curves, Grade, LutEffect, LutsView, Wheel } from './types';

export const wheel = (v: number): Wheel => ({ master: v, r: v, g: v, b: v });

export function defaultGrade(): Grade {
  return {
    exposure: 0,
    temperature: 0,
    tint: 0,
    lift: wheel(0),
    gamma: wheel(1),
    gain: wheel(1),
    contrast: 1,
    saturation: 1,
  };
}

export function defaultCurve(): Curve {
  return {
    points: [
      [0, 0],
      [1, 1],
    ],
  };
}

export function defaultCurves(): Curves {
  return { master: defaultCurve(), r: defaultCurve(), g: defaultCurve(), b: defaultCurve() };
}

/** Monotone cubic (Fritsch–Carlson), the same spline as lut-core's `Curve`; only for drawing the curve. */
export function curveFn(c: Curve): (x: number) => number {
  const p = [...c.points]
    .map(([x, y]) => [Math.min(1, Math.max(0, x)), Math.min(1, Math.max(0, y))] as [number, number])
    .sort((a, b) => a[0] - b[0]);
  const pts: [number, number][] = [];
  for (const q of p) {
    const last = pts[pts.length - 1];
    if (last && Math.abs(q[0] - last[0]) < 1e-6) pts[pts.length - 1] = q;
    else pts.push(q);
  }
  const n = pts.length;
  if (n === 0) return (x) => x;
  if (n === 1) return () => pts[0][1];
  const xs = pts.map((q) => q[0]);
  const ys = pts.map((q) => q[1]);
  const d = xs.slice(0, -1).map((x, i) => (ys[i + 1] - ys[i]) / (xs[i + 1] - x));
  const m = new Array<number>(n).fill(0);
  m[0] = d[0];
  m[n - 1] = d[n - 2];
  for (let i = 1; i < n - 1; i++) m[i] = d[i - 1] * d[i] <= 0 ? 0 : (d[i - 1] + d[i]) / 2;
  for (let i = 0; i < n - 1; i++) {
    if (d[i] === 0) {
      m[i] = 0;
      m[i + 1] = 0;
      continue;
    }
    const a = m[i] / d[i];
    const b = m[i + 1] / d[i];
    const s = a * a + b * b;
    if (s > 9) {
      const t = 3 / Math.sqrt(s);
      m[i] = t * a * d[i];
      m[i + 1] = t * b * d[i];
    }
  }
  return (x) => {
    if (x <= xs[0]) return ys[0];
    if (x >= xs[n - 1]) return ys[n - 1];
    let i = 0;
    while (i < n - 2 && x >= xs[i + 1]) i++;
    const h = xs[i + 1] - xs[i];
    const t = (x - xs[i]) / h;
    const t2 = t * t;
    const t3 = t2 * t;
    const y =
      (2 * t3 - 3 * t2 + 1) * ys[i] + (t3 - 2 * t2 + t) * h * m[i] + (-2 * t3 + 3 * t2) * ys[i + 1] + (t3 - t2) * h * m[i + 1];
    return Math.min(1, Math.max(0, y));
  };
}

/** Where a LUT effect sits, as the LUT tab shows it. */
export function placeLabel(e: LutEffect): string {
  const p = e.place;
  if (p.kind === 'composition') return 'Composition';
  if (p.kind === 'layer') return `Layer ${p.layer} · ${p.name || 'unnamed'}`;
  return `Clip ${p.layer}/${p.column} · ${p.name || 'unnamed'}`;
}

const norm = (s: string) => s.replace(/\//g, '\\').toLowerCase();

/** The library LUT whose .cube a LUT effect uses (Windows paths: case and slashes don't matter). */
export function lutOfEffect(e: LutEffect, luts: LutsView | null): string | null {
  if (!luts || !e.path) return null;
  const p = norm(e.path);
  return luts.luts.find((l) => norm(l.path) === p)?.name ?? null;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** `base`, else `base 2`, `base 3`, …: the first name not taken (Windows: case doesn't matter). */
export function freeLutName(luts: LutsView | null, base: string): string {
  const taken = new Set((luts?.luts ?? []).map((l) => l.name.toLowerCase()));
  const b = base.replace(/[<>:"/\\|?*\u0000-\u001f]/g, '').trim().replace(/^\.+|\.+$/g, '').slice(0, 56) || 'LUT';
  if (!taken.has(b.toLowerCase())) return b;
  for (let i = 2; ; i++) if (!taken.has(`${b} ${i}`.toLowerCase())) return `${b} ${i}`;
}

// ---- bundled test images: drawn ones, and the Kodak suite (web/public/kodak, see its README) ----

export type SampleImage =
  | { id: string; label: string; draw: (ctx: CanvasRenderingContext2D, w: number, h: number) => void }
  | { id: string; label: string; url: string };

/** Kodak Lossless True Color Image Suite: 24 photos (768 × 512, six portrait). */
export const KODAK_IMAGES: SampleImage[] = Array.from({ length: 24 }, (_, i) => {
  const n = String(i + 1).padStart(2, '0');
  return { id: `kodak${n}`, label: `Kodak ${n}`, url: `/kodak/kodim${n}.jpg` };
});

/** sRGB values of the 24 patches of a classic colour checker (published reference values, rounded). */
const CHECKER = [
  '#735244', '#c29682', '#627a9d', '#576c43', '#8580b1', '#67bdaa',
  '#d67e2c', '#505ba6', '#c15a63', '#5e3c6c', '#9dbc40', '#e0a32e',
  '#383d96', '#469449', '#af363c', '#e7c71f', '#bb5695', '#0885a1',
  '#f3f3f2', '#c8c8c8', '#a0a0a0', '#7a7a79', '#555555', '#343434',
];

export const SAMPLE_IMAGES: SampleImage[] = [
  {
    id: 'ramps',
    label: 'Ramps',
    draw(ctx, w, h) {
      const rows = ['#ffffff', '#ff0000', '#00ff00', '#0000ff'];
      const rh = h / 5;
      rows.forEach((c, i) => {
        const g = ctx.createLinearGradient(0, 0, w, 0);
        g.addColorStop(0, '#000000');
        g.addColorStop(1, c);
        ctx.fillStyle = g;
        ctx.fillRect(0, i * rh, w, rh);
      });
      const hue = ctx.createLinearGradient(0, 0, w, 0);
      for (let i = 0; i <= 6; i++) hue.addColorStop(i / 6, `hsl(${i * 60} 100% 50%)`);
      ctx.fillStyle = hue;
      ctx.fillRect(0, 4 * rh, w, rh);
    },
  },
  {
    id: 'chart',
    label: 'Colour chart',
    draw(ctx, w, h) {
      ctx.fillStyle = '#111111';
      ctx.fillRect(0, 0, w, h);
      const cw = w / 6;
      const ch = h / 4;
      CHECKER.forEach((c, i) => {
        ctx.fillStyle = c;
        ctx.fillRect((i % 6) * cw + cw * 0.06, Math.floor(i / 6) * ch + ch * 0.08, cw * 0.88, ch * 0.84);
      });
    },
  },
  {
    id: 'skin',
    label: 'Skin tones',
    draw(ctx, w, h) {
      const tones = ['#f6d7c3', '#eac0a5', '#d9a07f', '#c68863', '#a86d4c', '#8a5537', '#6b3f28', '#4a2a1b'];
      const g = ctx.createLinearGradient(0, 0, 0, h);
      g.addColorStop(0, '#2b3a55');
      g.addColorStop(1, '#0e1320');
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, w, h);
      tones.forEach((c, i) => {
        const x = (i % 4) * (w / 4) + w / 8;
        const y = Math.floor(i / 4) * (h / 2) + h / 4;
        const r = Math.min(w / 10, h / 5);
        const rg = ctx.createRadialGradient(x - r * 0.3, y - r * 0.3, r * 0.1, x, y, r);
        rg.addColorStop(0, '#ffffff55');
        rg.addColorStop(0.25, c);
        rg.addColorStop(1, '#00000000');
        ctx.fillStyle = c;
        ctx.beginPath();
        ctx.arc(x, y, r, 0, Math.PI * 2);
        ctx.fill();
        ctx.fillStyle = rg;
        ctx.fill();
      });
    },
  },
  {
    id: 'neon',
    label: 'Neon / LED',
    draw(ctx, w, h) {
      ctx.fillStyle = '#000000';
      ctx.fillRect(0, 0, w, h);
      const colors = ['#ff00aa', '#00e5ff', '#7cff00', '#ffb300', '#7a00ff', '#ff2a2a'];
      ctx.lineCap = 'round';
      colors.forEach((c, i) => {
        const y = ((i + 0.5) / colors.length) * h;
        for (const [width, alpha] of [[28, '22'], [14, '55'], [5, 'ff']] as const) {
          ctx.strokeStyle = c + alpha;
          ctx.lineWidth = (width * h) / 400;
          ctx.beginPath();
          ctx.moveTo(w * 0.08, y);
          ctx.bezierCurveTo(w * 0.35, y - h * 0.12, w * 0.65, y + h * 0.12, w * 0.92, y);
          ctx.stroke();
        }
      });
    },
  },
];

// ---- WebGL2 preview ----

const VERT = `#version 300 es
in vec2 pos;
out vec2 uv;
void main() { uv = pos * 0.5 + 0.5; gl_Position = vec4(pos, 0.0, 1.0); }`;

const FRAG = `#version 300 es
precision highp float;
precision highp sampler3D;
uniform sampler2D img;
uniform sampler3D lut;
uniform float size;
uniform bool hasLut;
uniform vec2 canvasPx;
uniform float gapPx;
uniform float imgAspect;
in vec2 uv;
out vec4 color;
void main() {
  // Two panels side by side: left the original, right with the LUT. Each shows the whole image, fitted.
  float panelW = (canvasPx.x - gapPx) * 0.5;
  vec2 p = gl_FragCoord.xy;
  bool right = p.x >= panelW + gapPx;
  if (!right && p.x >= panelW) discard;
  float x = right ? p.x - panelW - gapPx : p.x;
  vec2 fit = panelW / canvasPx.y > imgAspect
    ? vec2(canvasPx.y * imgAspect, canvasPx.y)
    : vec2(panelW, panelW / imgAspect);
  vec2 off = (vec2(panelW, canvasPx.y) - fit) * 0.5;
  vec2 q = (vec2(x, p.y) - off) / fit;
  if (q.x < 0.0 || q.x > 1.0 || q.y < 0.0 || q.y > 1.0) discard;
  vec4 c = texture(img, vec2(q.x, 1.0 - q.y));
  if (hasLut && right) {
    vec3 s = clamp(c.rgb, 0.0, 1.0) * ((size - 1.0) / size) + 0.5 / size;
    c.rgb = texture(lut, s).rgb;
  }
  color = c;
}`;

/** Gap between the two panels, in canvas pixels. */
const GAP_PX = 12;

/**
 * Draws an image twice, side by side (SPEC §14.4): the original on the left, with the LUT applied on the right; each
 * fitted into its half with its aspect kept. Outside the images the canvas stays transparent.
 */
export class LutPreview {
  private gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private img: WebGLTexture;
  private lut: WebGLTexture;
  private size = 0;
  private hasImage = false;
  private aspect = 16 / 9;

  constructor(canvas: HTMLCanvasElement) {
    const gl = canvas.getContext('webgl2', { premultipliedAlpha: false, preserveDrawingBuffer: true });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    const sh = (type: number, src: string) => {
      const s = gl.createShader(type)!;
      gl.shaderSource(s, src);
      gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader');
      return s;
    };
    const prog = gl.createProgram()!;
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) ?? 'program');
    this.prog = prog;
    const buf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(prog, 'pos');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    this.img = gl.createTexture()!;
    this.lut = gl.createTexture()!;
    gl.bindTexture(gl.TEXTURE_3D, this.lut);
    for (const p of [gl.TEXTURE_WRAP_S, gl.TEXTURE_WRAP_T, gl.TEXTURE_WRAP_R]) gl.texParameteri(gl.TEXTURE_3D, p, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  }

  setImage(src: HTMLImageElement | HTMLCanvasElement) {
    const gl = this.gl;
    const w = src instanceof HTMLImageElement ? src.naturalWidth : src.width;
    const h = src instanceof HTMLImageElement ? src.naturalHeight : src.height;
    this.aspect = w > 0 && h > 0 ? w / h : 16 / 9;
    gl.bindTexture(gl.TEXTURE_2D, this.img);
    gl.pixelStorei(gl.UNPACK_COLORSPACE_CONVERSION_WEBGL, gl.NONE);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, src);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    this.hasImage = true;
  }

  /** `data`: size³ RGB floats, red fastest (GET /api/luts/table). `null` shows the image unchanged. */
  setTable(size: number, data: Float32Array | null) {
    const gl = this.gl;
    if (!data) {
      this.size = 0;
      return;
    }
    gl.bindTexture(gl.TEXTURE_3D, this.lut);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    gl.texImage3D(gl.TEXTURE_3D, 0, gl.RGB16F, size, size, size, 0, gl.RGB, gl.FLOAT, data);
    this.size = size;
  }

  draw() {
    const gl = this.gl;
    const c = gl.canvas as HTMLCanvasElement;
    gl.viewport(0, 0, c.width, c.height);
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (!this.hasImage) return;
    gl.useProgram(this.prog);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.img);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_3D, this.lut);
    gl.uniform1i(gl.getUniformLocation(this.prog, 'img'), 0);
    gl.uniform1i(gl.getUniformLocation(this.prog, 'lut'), 1);
    gl.uniform1f(gl.getUniformLocation(this.prog, 'size'), this.size || 2);
    gl.uniform2f(gl.getUniformLocation(this.prog, 'canvasPx'), c.width, c.height);
    gl.uniform1f(gl.getUniformLocation(this.prog, 'gapPx'), GAP_PX);
    gl.uniform1f(gl.getUniformLocation(this.prog, 'imgAspect'), this.aspect);
    gl.uniform1i(gl.getUniformLocation(this.prog, 'hasLut'), this.size > 0 ? 1 : 0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
}

/** Fetches a LUT's baked table: `{ rev, size, data }`. */
export async function fetchTable(name: string): Promise<{ rev: number; size: number; data: Float32Array } | null> {
  const r = await fetch(`/api/luts/table?name=${encodeURIComponent(name)}`);
  if (!r.ok) return null;
  const size = Number(r.headers.get('x-lut-size'));
  const rev = Number(r.headers.get('x-lut-rev'));
  const data = new Float32Array(await r.arrayBuffer());
  if (!size || data.length !== size * size * size * 3) return null;
  return { rev, size, data };
}
