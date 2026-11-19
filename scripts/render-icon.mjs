// 已废弃：应用图标改用设计素材包 Buildship_AuroraLoop（见 README「打包」章节）。
// 保留仅为追溯最早的图标生成流程。
// 把 scripts/icon.svg 渲染成 1024×1024 的应用图标源图。
// sharp 是 apps/desktop 的开发依赖，这里用 createRequire 从该包解析，避免根目录再装一份。
// 用法：node scripts/render-icon.mjs
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, "..", "apps", "desktop", "package.json"));
const sharp = require("sharp");

const svg = path.join(here, "icon.svg");
const target = path.join(here, "..", "apps", "desktop", "src-tauri", "icons", "source.png");

// 渲染尺寸与 SVG 声明的 1024 一致，避免缩放带来的振铃（圆角处出现亮边）
await sharp(svg, { density: 96 })
  .resize(1024, 1024, { fit: "contain", kernel: "lanczos3", background: { r: 0, g: 0, b: 0, alpha: 0 } })
  .png({ compressionLevel: 9 })
  .toFile(target);

console.log(`已生成 ${target}`);
