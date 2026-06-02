use screen_sidekick_safety::{PromptSafeText, SanitizedUrl};

fn main() {
    let _text = PromptSafeText("secret".to_owned());
    let _url = SanitizedUrl("https://example.test/?token=secret".to_owned());
    let _text = PromptSafeText::new_unchecked("secret".to_owned());
    let _url = SanitizedUrl::new_unchecked("https://example.test/?token=secret".to_owned());
}
