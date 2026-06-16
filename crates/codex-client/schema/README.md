# Codex App-Server Schema Snapshot

This directory is the committed compatibility snapshot for the Phase 1 app-server client.

Normal build and test commands must not invoke the local `codex` binary.
Use explicit developer commands instead:

```text
make codex-schema-refresh
make codex-schema-check
make codex-schema-metadata
```

The Rust runtime client intentionally uses a handwritten Phase 1 subset rather than generated Rust bindings for every app-server field.
