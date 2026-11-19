import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { fileURLToPath, URL } from "node:url";

// Tauri 需要固定端口 + 明确的产物目录；同时构建两个入口：
// index.html（主面板）与 floating.html（悬浮栏）。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    // Windows 用 WebView2、macOS 用 WKWebView，都支持现代语法
    target: "chrome110",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        floating: fileURLToPath(new URL("./floating.html", import.meta.url)),
      },
    },
  },
});
