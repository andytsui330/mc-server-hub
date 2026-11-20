import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen } from "@tauri-apps/api/event";

// 外殼只提供資料夾選取與視窗事件，畫面不直接碰檔案或程序。
export function pickFolder(): Promise<string | null> {
  return invoke("pick_folder");
}

export function closeWindow(): Promise<void> {
  return invoke("close_window");
}

export function onFolderDrop(handler: (paths: string[]) => void): Promise<() => void> {
  return getCurrentWebviewWindow().onDragDropEvent(event => {
    if (event.payload.type === "drop") handler(event.payload.paths);
  });
}

export function onCloseBlocked(handler: () => void): Promise<() => void> {
  return listen("close-blocked", handler);
}

export function onCloseAsk(handler:()=>void):Promise<()=>void>{return listen('close-ask',handler)}

export function onDraftClose(handler:()=>void):Promise<()=>void>{return listen('draft-close-requested',handler)}
