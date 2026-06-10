import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const extensionDir = dirname(fileURLToPath(import.meta.url));
const manifest = JSON.parse(readFileSync(join(extensionDir, "manifest.json"), "utf8"));
const worker = manifest.background?.service_worker;

if (worker !== "dist/background.js") {
  throw new Error("MV3 service_worker must point to dist/background.js");
}

if (manifest.side_panel?.default_path !== "side_panel.html") {
  throw new Error("side panel default path must be side_panel.html");
}

for (const permission of ["activeTab", "scripting", "sidePanel", "storage", "tabs"]) {
  if (!manifest.permissions?.includes(permission)) {
    throw new Error(`missing permission: ${permission}`);
  }
}

for (const origin of ["http://*/*", "https://*/*"]) {
  if (!manifest.optional_host_permissions?.includes(origin)) {
    throw new Error(`missing optional host permission: ${origin}`);
  }
}

for (const path of [
  "dist/background.js",
  "dist/side_panel.js",
  "side_panel.html",
]) {
  const absolutePath = join(extensionDir, path);
  if (!existsSync(absolutePath)) {
    throw new Error(`missing built extension asset: ${path}`);
  }
}
