import { invoke } from "@tauri-apps/api/core";

// 畫面呼叫後端的唯一入口（計畫書第三節原則 1）：路徑與回傳格式照 1.x 的 /api/…，後端在 core/src/hub.rs。
export type Reply = { ok?: true; error?: string; help_url?: string | null; need?: "eula"; [k: string]: unknown };

export function api<T = Reply>(path: string, body: Record<string, unknown> = {}): Promise<T> {
  return invoke<T>("api", { path, body });
}
