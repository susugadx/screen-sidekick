#!/usr/bin/env bash
set -euo pipefail

committed_dir="crates/codex-client/schema/app-server"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

codex app-server generate-json-schema --out "$tmp_dir"

committed_hash="$(node crates/codex-client/schema/hash_schema.mjs "$committed_dir")"
observed_hash="$(node crates/codex-client/schema/hash_schema.mjs "$tmp_dir")"

if [ "$committed_hash" != "$observed_hash" ]; then
  echo "Codex app-server schema drift detected."
  echo "Observed Codex version: $(codex --version)"
  echo "committed: $committed_hash"
  echo "observed:  $observed_hash"
  exit 1
fi

metadata_hash="$(sed -n 's/.*"schema_hash": "sha256:\([0-9a-f]*\)".*/\1/p' crates/codex-client/schema/metadata.json)"

if [ "$committed_hash" != "$metadata_hash" ]; then
  echo "Codex app-server schema metadata hash is stale."
  echo "metadata: $metadata_hash"
  echo "actual:   $committed_hash"
  exit 1
fi

echo "Codex app-server schema snapshot matches $(codex --version)."
