/**
 * 构建日志的 ANSI（SGR）着色解析。
 *
 * npm / yarn / vite 等工具在非 TTY 下仍会输出 `\x1b[94m…\x1b[39m` 形式的颜色码，
 * 直接当文本渲染就会变成用户看到的「乱码」。这里把 SGR 转成内联样式，
 * 其余 CSI / OSC 控制序列（清行、光标定位、标题）一律丢弃。
 *
 * 解析结果按「任务 + 行号」缓存：日志实时追加时只解析新行。
 */

export interface AnsiSpan {
  text: string;
  style?: string;
}

/** 与深色控制台背景匹配的 16 色调色板 */
const FG: Record<number, string> = {
  30: "#5b6a80",
  31: "#ff7b72",
  32: "#7ee787",
  33: "#f2cc60",
  34: "#79b8ff",
  35: "#d2a8ff",
  36: "#56d4dd",
  37: "#e6edf3",
  90: "#8b98a9",
  91: "#ffa198",
  92: "#a5f3b6",
  93: "#ffdf8a",
  94: "#9ecbff",
  95: "#e2c5ff",
  96: "#99e9ee",
  97: "#ffffff",
};

const BG: Record<number, string> = {
  40: "#1b2436",
  41: "#7d2a2a",
  42: "#1d6b33",
  43: "#7a5a12",
  44: "#1e3f78",
  45: "#5b2d78",
  46: "#155a63",
  47: "#d7e3f4",
  100: "#2b3548",
  101: "#a33a3a",
  102: "#2ea043",
  103: "#a9821a",
  104: "#2f5ea8",
  105: "#7b3fa0",
  106: "#1f7b86",
  107: "#ffffff",
};

const OSC_RE = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
const CSI_OTHER_RE = /\x1b\[[0-9;?]*[ -/]*[@-~]/g;
const SGR_RE = /\x1b\[([0-9;]*)m/y;

interface SgrState {
  fg: string | null;
  bg: string | null;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
  inverse: boolean;
}

function emptyState(): SgrState {
  return { fg: null, bg: null, bold: false, dim: false, italic: false, underline: false, inverse: false };
}

function applySgr(state: SgrState, params: string): void {
  const codes = params.length === 0 ? [0] : params.split(";").map((part) => Number.parseInt(part, 10) || 0);
  for (let i = 0; i < codes.length; i += 1) {
    const code = codes[i];
    if (code === 0) {
      Object.assign(state, emptyState());
    } else if (code === 1) {
      state.bold = true;
    } else if (code === 2) {
      state.dim = true;
    } else if (code === 3) {
      state.italic = true;
    } else if (code === 4) {
      state.underline = true;
    } else if (code === 7) {
      state.inverse = true;
    } else if (code === 22) {
      state.bold = false;
      state.dim = false;
    } else if (code === 23) {
      state.italic = false;
    } else if (code === 24) {
      state.underline = false;
    } else if (code === 27) {
      state.inverse = false;
    } else if (FG[code]) {
      state.fg = FG[code];
    } else if (BG[code]) {
      state.bg = BG[code];
    } else if (code === 39) {
      state.fg = null;
    } else if (code === 49) {
      state.bg = null;
    } else if (code === 38 || code === 48) {
      // 256 色 / 真彩色：38;5;n 或 38;2;r;g;b
      const mode = codes[i + 1];
      if (mode === 5) {
        const color = xterm256(codes[i + 2]);
        if (code === 38) state.fg = color;
        else state.bg = color;
        i += 2;
      } else if (mode === 2) {
        const color = `rgb(${codes[i + 2] ?? 0}, ${codes[i + 3] ?? 0}, ${codes[i + 4] ?? 0})`;
        if (code === 38) state.fg = color;
        else state.bg = color;
        i += 4;
      }
    }
  }
}

function xterm256(index: number | undefined): string {
  if (index === undefined) return "#d7e3f4";
  if (index < 16) {
    const table = Object.values(FG);
    return table[index] ?? "#d7e3f4";
  }
  if (index < 232) {
    const i = index - 16;
    const r = Math.floor(i / 36);
    const g = Math.floor((i % 36) / 6);
    const b = i % 6;
    const to = (v: number) => (v === 0 ? 0 : 55 + v * 40);
    return `rgb(${to(r)}, ${to(g)}, ${to(b)})`;
  }
  const gray = 8 + (index - 232) * 10;
  return `rgb(${gray}, ${gray}, ${gray})`;
}

function styleOf(state: SgrState): string | undefined {
  const parts: string[] = [];
  let fg = state.fg;
  let bg = state.bg;
  if (state.inverse) {
    fg = state.bg ?? "#0f1a2b";
    bg = state.fg ?? "#d7e3f4";
  }
  if (fg) parts.push(`color: ${fg}`);
  if (bg) parts.push(`background: ${bg}`, "border-radius: 3px");
  if (state.bold) parts.push("font-weight: 600");
  if (state.dim) parts.push("opacity: 0.72");
  if (state.italic) parts.push("font-style: italic");
  if (state.underline) parts.push("text-decoration: underline");
  return parts.length ? parts.join("; ") : undefined;
}

/** 解析一行日志，返回可渲染的片段（无控制序列时只有一段）。 */
export function parseAnsi(input: string): AnsiSpan[] {
  if (!input) return [];
  // 去掉 OSC（窗口标题、超链接等）与其它 CSI，只留 SGR 供着色
  let text = input.includes("\x1b") ? input.replace(OSC_RE, "").replace(/\x1b\][^\x07]*$/, "") : input;
  if (text.includes("\r")) {
    // 进度条会用 \r 回到行首覆盖，取最后一段才是最终可见内容
    text = text.slice(text.lastIndexOf("\r") + 1);
  }
  if (!text.includes("\x1b")) return text ? [{ text }] : [];

  const spans: AnsiSpan[] = [];
  const state = emptyState();
  let cursor = 0;

  const push = (chunk: string) => {
    if (!chunk) return;
    const style = styleOf(state);
    spans.push(style ? { text: chunk, style } : { text: chunk });
  };

  while (cursor < text.length) {
    const next = text.indexOf("\x1b", cursor);
    if (next < 0) {
      push(text.slice(cursor));
      break;
    }
    push(text.slice(cursor, next));

    SGR_RE.lastIndex = next;
    const match = SGR_RE.exec(text);
    if (match) {
      applySgr(state, match[1] ?? "");
      cursor = SGR_RE.lastIndex;
      continue;
    }

    // 非 SGR 的转义序列直接跳过
    CSI_OTHER_RE.lastIndex = next;
    const csi = CSI_OTHER_RE.exec(text);
    if (csi && csi.index === next) {
      cursor = CSI_OTHER_RE.lastIndex;
      continue;
    }
    if (text[next + 1] === "]") {
      const end = text.indexOf("\x07", next);
      cursor = end < 0 ? text.length : end + 1;
      continue;
    }
    // 单个 ESC 后跟一个字节，跳过
    cursor = next + 2;
  }

  return spans.filter((span) => span.text.length > 0);
}

const cache = new Map<string, AnsiSpan[]>();
const CACHE_LIMIT = 20000;

/** 带缓存的行解析：key 建议用 `${taskId}:${seq}`。 */
export function ansiSpans(key: string, text: string): AnsiSpan[] {
  const hit = cache.get(key);
  if (hit) return hit;
  const spans = parseAnsi(text);
  if (cache.size >= CACHE_LIMIT) cache.clear();
  cache.set(key, spans);
  return spans;
}

/** 日志换任务时清空缓存，避免内存随任务数累积。 */
export function clearAnsiCache(): void {
  cache.clear();
}
