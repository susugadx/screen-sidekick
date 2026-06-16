#!/usr/bin/env node
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const root = process.argv[2];
if (!root) {
  console.error("usage: hash_schema.mjs <schema-dir>");
  process.exit(2);
}

const files = listJsonFiles(root).sort();
const hash = createHash("sha256");
for (const file of files) {
  const relativePath = relative(root, file);
  const canonical = canonicalize(JSON.parse(readFileSync(file, "utf8")));
  hash.update(relativePath);
  hash.update("\0");
  hash.update(JSON.stringify(canonical));
  hash.update("\0");
}

process.stdout.write(hash.digest("hex"));

function listJsonFiles(directory) {
  const files = [];
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    const stats = statSync(path);
    if (stats.isDirectory()) {
      files.push(...listJsonFiles(path));
    } else if (entry.endsWith(".json")) {
      files.push(path);
    }
  }
  return files;
}

function canonicalize(value) {
  if (Array.isArray(value)) {
    return value.map(canonicalize);
  }
  if (value && typeof value === "object") {
    const canonical = {};
    for (const key of Object.keys(value).sort()) {
      canonical[key] = canonicalize(value[key]);
    }
    return canonical;
  }
  return value;
}
