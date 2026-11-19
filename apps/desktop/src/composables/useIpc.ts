import { invoke } from "@tauri-apps/api/core";
import type { IpcError } from "@/types/ipc";

/** 统一错误包装：把 Rust 的 { code, message, detail } 变成可判定的异常 */
export class IpcCallError extends Error {
  code: string;
  detail?: string;

  constructor(payload: IpcError) {
    super(payload.message);
    this.name = "IpcCallError";
    this.code = payload.code;
    this.detail = payload.detail;
  }
}

function normalizeError(raw: unknown): IpcCallError {
  if (raw && typeof raw === "object" && "message" in raw) {
    const payload = raw as Partial<IpcError>;
    return new IpcCallError({
      code: payload.code ?? "INTERNAL",
      message: payload.message ?? "未知错误",
      detail: payload.detail,
    });
  }
  if (typeof raw === "string") {
    // Tauri 在命令 panic 时返回字符串
    return new IpcCallError({ code: "INTERNAL", message: raw });
  }
  return new IpcCallError({ code: "INTERNAL", message: "调用失败，请查看应用日志" });
}

/** 是否运行在 Tauri 容器内（浏览器里直接开 dev server 时为 false） */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 调用 Rust 命令 */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    throw new IpcCallError({
      code: "UNIMPLEMENTED",
      message: "当前不在桌面应用内运行，无法调用本地能力",
      detail: "请使用 npm run tauri:dev 启动桌面端",
    });
  }
  try {
    return await invoke<T>(command, args);
  } catch (raw) {
    throw normalizeError(raw);
  }
}

/** 把错误转成一句可展示文案 */
export function errorText(error: unknown): string {
  if (error instanceof IpcCallError) {
    return error.detail ? `${error.message}（${error.detail}）` : error.message;
  }
  if (error instanceof Error) return error.message;
  return String(error);
}
