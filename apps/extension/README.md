# Browser Extension Scaffold

This directory is a placeholder for the Chrome/Edge adapter.

The extension may own:

- Chrome extension entrypoints.
- Chrome API calls.
- DOM capture entrypoints.
- Screenshot and selected text capture entrypoints.
- Side panel UI wiring.

The extension must not own:

- `ScreenContext` schema policy.
- Danger detection.
- Secret or input masking.
- Prompt generation.
- Handoff execution.
- Browser automation.

Phase 0-A has no TypeScript package, bundler, or type check. Manifest-wired
entrypoints must be loadable JavaScript files, so the MV3 service worker points
to `src/background.js`. The TypeScript files are intentionally thin, unwired
entrypoint placeholders until build tooling exists.
