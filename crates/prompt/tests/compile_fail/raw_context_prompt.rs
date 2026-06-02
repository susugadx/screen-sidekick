use screen_sidekick_prompt::build_codex_prompt;
use screen_sidekick_screen_context::RawScreenContext;

fn main() {
    let context = RawScreenContext::new();
    let _prompt = build_codex_prompt(&context);
}
