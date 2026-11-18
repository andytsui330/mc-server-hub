import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 開發模式固定連 5173；埠被占用就報錯，不要偷換埠
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
});
