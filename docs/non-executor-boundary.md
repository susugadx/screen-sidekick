# Non-Executor Boundary

Screen Sidekick is a context packaging and handoff application. It must not
become an executor.

## Sidekick May

- Capture visual context.
- Normalize context into `ScreenContext`.
- Mask input values and secret-like selected text.
- Warn about dangerous actions.
- Generate a prompt preview.
- Build a handoff package for a user to review.

## Sidekick Must Not

- Click buttons or submit forms.
- Run browser automation.
- Run MCP tools.
- Edit local repositories.
- Execute tests or shell commands on behalf of the captured page.
- Send prompts to Codex automatically.
- Store or forward raw DOM, cookies, localStorage, sessionStorage, hidden input
  values, passwords, tokens, API keys, card numbers, or 2FA codes.

## Dangerous Action Warnings

The safety layer must warn before handoff content involves:

- delete, remove, or destroy
- publish
- send or submit
- billing, payment, or charge
- permission, admin, or owner changes
- revoke, disconnect, or reset
- secret, token, key, or password changes

These warnings are previews only. They do not authorize Sidekick to execute the
action.
