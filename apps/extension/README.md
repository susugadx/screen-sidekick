# Browser Extension

This directory owns the Chrome/Edge side panel adapter.

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

## Checks

```sh
npm install
npm run typecheck
npm run build
npm test
node check-manifest.mjs
```

Build before loading the directory as an unpacked extension. The generated
`dist/` directory is intentionally ignored by git.
