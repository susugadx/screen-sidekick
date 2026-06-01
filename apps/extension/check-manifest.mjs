import { readFileSync } from "node:fs";

const manifest = JSON.parse(readFileSync("apps/extension/manifest.json", "utf8"));
const worker = manifest.background?.service_worker;

if (!worker?.endsWith(".js")) {
  throw new Error("MV3 service_worker must point to a .js file in Phase 0-A");
}
