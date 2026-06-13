#!/usr/bin/env bash
set -euo pipefail

schema_dir="crates/codex-client/schema/app-server"
metadata_file="crates/codex-client/schema/metadata.json"

rm -rf "$schema_dir"
mkdir -p "$schema_dir"
codex app-server generate-json-schema --out "$schema_dir"

schema_hash="$(node crates/codex-client/schema/hash_schema.mjs "$schema_dir")"
codex_version="$(codex --version)"
generated_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

tmp_file="$(mktemp)"
cat >"$tmp_file" <<JSON
{
  "codex_cli_version": "$codex_version",
  "generated_at": "$generated_at",
  "generation_command": "codex app-server generate-json-schema --out crates/codex-client/schema/app-server",
  "experimental": false,
  "schema_hash": "sha256:$schema_hash",
  "notes": [
    "Generated for the Phase 1 Chat-First Browser MVP app-server subset."
  ]
}
JSON
mv "$tmp_file" "$metadata_file"
