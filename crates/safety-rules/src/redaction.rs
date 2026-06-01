use screen_sidekick_screen_context::MASKED_VALUE;
use url::{form_urlencoded, Url};

pub const REDACTED_URL_VALUE: &str = "[REDACTED]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextMaskResult {
    pub text: String,
    pub was_masked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretKeyClass {
    AlwaysRedact,
    ValueAware,
}

#[must_use]
pub fn mask_secret_like_text(text: &str) -> TextMaskResult {
    if contains_secret_signal(text) {
        TextMaskResult {
            text: MASKED_VALUE.to_owned(),
            was_masked: true,
        }
    } else {
        TextMaskResult {
            text: text.to_owned(),
            was_masked: false,
        }
    }
}

#[must_use]
pub fn redact_secret_bearing_url(url: &str) -> TextMaskResult {
    match Url::parse(url) {
        Ok(parsed_url) => redact_parsed_url(&parsed_url),
        Err(_) => {
            if contains_secret_signal(url) {
                TextMaskResult {
                    text: MASKED_VALUE.to_owned(),
                    was_masked: true,
                }
            } else {
                TextMaskResult {
                    text: url.to_owned(),
                    was_masked: false,
                }
            }
        }
    }
}

fn redact_parsed_url(url: &Url) -> TextMaskResult {
    let userinfo_was_redacted = url_has_userinfo(url);
    let (redacted_path, path_was_redacted) = redact_url_path(url.path());
    let (redacted_query, query_was_redacted) = redact_pair_list(url.query());
    let (redacted_fragment, fragment_was_redacted) = redact_fragment(url.fragment());

    if !userinfo_was_redacted && !path_was_redacted && !query_was_redacted && !fragment_was_redacted
    {
        return TextMaskResult {
            text: url.as_str().to_owned(),
            was_masked: false,
        };
    }

    let mut base_url = url.clone();
    base_url.set_query(None);
    base_url.set_fragment(None);
    if userinfo_was_redacted && !remove_url_userinfo(&mut base_url) {
        return TextMaskResult {
            text: MASKED_VALUE.to_owned(),
            was_masked: true,
        };
    }

    let mut text = base_url.to_string();
    if path_was_redacted {
        text = text.replacen(url.path(), &redacted_path, 1);
    }
    if let Some(query) = redacted_query {
        text.push('?');
        text.push_str(&query);
    }
    if let Some(fragment) = redacted_fragment {
        text.push('#');
        text.push_str(&fragment);
    }

    TextMaskResult {
        text,
        was_masked: true,
    }
}

fn url_has_userinfo(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

fn remove_url_userinfo(url: &mut Url) -> bool {
    url.set_password(None).is_ok() && url.set_username("").is_ok()
}

fn redact_url_path(path: &str) -> (String, bool) {
    let mut was_redacted = false;
    let redacted_path = path
        .split('/')
        .map(|segment| {
            if segment.is_empty() {
                String::new()
            } else if is_secret_bearing_url_value(segment) {
                was_redacted = true;
                REDACTED_URL_VALUE.to_owned()
            } else {
                segment.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("/");

    (redacted_path, was_redacted)
}

fn redact_fragment(fragment: Option<&str>) -> (Option<String>, bool) {
    match fragment {
        Some(fragment) => {
            let (redacted_fragment, was_redacted) = match fragment.split_once('?') {
                Some((route, query_like)) => {
                    let (redacted_route, route_was_redacted) = redact_fragment_route(route);
                    let (redacted_query_like, query_was_redacted) =
                        redact_pair_list(Some(query_like));
                    let redacted_query_like =
                        redacted_query_like.unwrap_or_else(|| query_like.to_owned());

                    (
                        format!("{redacted_route}?{redacted_query_like}"),
                        route_was_redacted || query_was_redacted,
                    )
                }
                None => {
                    if is_hash_router_route(fragment) {
                        redact_fragment_route(fragment)
                    } else {
                        let (redacted_fragment, fragment_was_redacted) =
                            redact_pair_list(Some(fragment));
                        match redacted_fragment {
                            Some(redacted_fragment) => (redacted_fragment, fragment_was_redacted),
                            None => (fragment.to_owned(), false),
                        }
                    }
                }
            };

            (Some(redacted_fragment), was_redacted)
        }
        None => (None, false),
    }
}

fn is_hash_router_route(fragment: &str) -> bool {
    fragment.starts_with('/')
}

fn redact_fragment_route(route: &str) -> (String, bool) {
    if is_secret_bearing_url_value(route) {
        (REDACTED_URL_VALUE.to_owned(), true)
    } else {
        (route.to_owned(), false)
    }
}

fn redact_pair_list(raw_pairs: Option<&str>) -> (Option<String>, bool) {
    match raw_pairs {
        Some(raw_pairs) => {
            let mut was_redacted = false;
            let redacted = raw_pairs
                .split('&')
                .map(|pair| redact_pair(pair, &mut was_redacted))
                .collect::<Vec<_>>()
                .join("&");

            (Some(redacted), was_redacted)
        }
        None => (None, false),
    }
}

fn redact_pair(pair: &str, was_redacted: &mut bool) -> String {
    let (raw_key, raw_value) = match pair.split_once('=') {
        Some((raw_key, raw_value)) => (raw_key, Some(raw_value)),
        None => (pair, None),
    };

    let key_was_redacted = should_redact_pair_key(raw_key);
    let value_was_redacted = should_redact_pair_value(raw_key, raw_value);

    if key_was_redacted || value_was_redacted {
        *was_redacted = true;
        let redacted_key = if key_was_redacted {
            REDACTED_URL_VALUE
        } else {
            raw_key
        };
        match raw_value {
            Some(raw_value) => {
                let redacted_value = if value_was_redacted {
                    REDACTED_URL_VALUE
                } else {
                    raw_value
                };
                format!("{redacted_key}={redacted_value}")
            }
            None => REDACTED_URL_VALUE.to_owned(),
        }
    } else {
        pair.to_owned()
    }
}

fn should_redact_pair_key(raw_key: &str) -> bool {
    is_secret_bearing_url_value(raw_key)
}

fn should_redact_pair_value(raw_key: &str, raw_value: Option<&str>) -> bool {
    match classify_secret_bearing_url_key(raw_key) {
        Some(SecretKeyClass::AlwaysRedact) => raw_value.is_some(),
        Some(SecretKeyClass::ValueAware) => raw_value.is_some_and(is_value_aware_secret_value),
        None => match raw_value {
            Some(raw_value) => is_secret_bearing_url_value(raw_value),
            None => is_secret_bearing_url_value(raw_key),
        },
    }
}

fn classify_secret_bearing_url_key(raw_key: &str) -> Option<SecretKeyClass> {
    let key = decode_form_key(raw_key);
    let normalized_key = normalize_secret_key(&key);

    if URL_EXACT_ALWAYS_REDACT_SECRET_KEY_NAMES.contains(&normalized_key.as_str()) {
        Some(SecretKeyClass::AlwaysRedact)
    } else {
        classify_normalized_prompt_text_secret_key(&normalized_key)
    }
}

fn decode_form_key(raw_key: &str) -> String {
    let pair = format!("{raw_key}=");
    match form_urlencoded::parse(pair.as_bytes()).next() {
        Some((key, _)) => key.into_owned(),
        None => raw_key.to_owned(),
    }
}

fn is_secret_bearing_url_value(raw_value: &str) -> bool {
    if url_value_layer_has_secret(raw_value) {
        return true;
    }

    let mut current_value = raw_value.to_owned();
    for _ in 0..MAX_URL_VALUE_DECODE_DEPTH {
        let decoded_value = decode_form_value(&current_value);
        if decoded_value == current_value {
            return false;
        }

        if url_value_layer_has_secret(&decoded_value) {
            return true;
        }

        current_value = decoded_value;
    }

    // 深く encode された値は安全に検査しきれないため、出力前に値ごと redact する。
    true
}

fn url_value_layer_has_secret(value: &str) -> bool {
    contains_value_only_secret_signal(value)
        || contains_secret_key_assignment(value)
        || decoded_url_value_has_secret(value)
        || decoded_query_like_text_has_secret_assignment(value)
}

fn decode_form_value(raw_value: &str) -> String {
    let pair = format!("value={raw_value}");
    match form_urlencoded::parse(pair.as_bytes()).next() {
        Some((_, value)) => value.into_owned(),
        None => raw_value.to_owned(),
    }
}

fn decoded_url_value_has_secret(decoded_value: &str) -> bool {
    Url::parse(decoded_value)
        .map(|nested_url| redact_parsed_url(&nested_url).was_masked)
        .unwrap_or(false)
}

fn decoded_query_like_text_has_secret_assignment(decoded_value: &str) -> bool {
    decoded_value
        .split(['?', '#', '&'])
        .filter_map(|part| part.split_once('='))
        .any(|(raw_key, raw_value)| {
            should_redact_pair_key(raw_key) || should_redact_pair_value(raw_key, Some(raw_value))
        })
}

fn contains_secret_signal(text: &str) -> bool {
    text_layer_has_secret(text) || decoded_text_has_secret(text)
}

fn text_layer_has_secret(text: &str) -> bool {
    contains_value_only_secret_signal(text)
        || contains_secret_key_assignment(text)
        || contains_secret_label_value_sequence(text)
}

fn decoded_text_has_secret(text: &str) -> bool {
    let mut current_text = text.to_owned();
    for _ in 0..MAX_URL_VALUE_DECODE_DEPTH {
        let decoded_text = decode_form_value(&current_text);
        if decoded_text == current_text {
            return false;
        }

        if text_layer_has_secret(&decoded_text)
            || decoded_url_value_has_secret(&decoded_text)
            || decoded_query_like_text_has_secret_assignment(&decoded_text)
        {
            return true;
        }

        current_text = decoded_text;
    }

    true
}

fn contains_value_only_secret_signal(text: &str) -> bool {
    contains_unkeyed_secret_text_signal(text)
        || contains_card_like_digit_sequence(text)
        || contains_secret_like_value_token(text)
}

fn contains_unkeyed_secret_text_signal(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();

    SECRET_TEXT_SIGNALS
        .iter()
        .any(|signal| normalized.contains(signal))
}

fn contains_secret_key_assignment(text: &str) -> bool {
    text.char_indices()
        .filter(|(_, character)| matches!(character, '=' | ':'))
        .any(|(separator_index, separator)| {
            let key = assignment_key_before_separator(&text[..separator_index]);
            let value_start = separator_index + separator.len_utf8();
            secret_key_assignment_is_secret(key, &text[value_start..])
        })
}

fn secret_key_assignment_is_secret(raw_key: &str, raw_value: &str) -> bool {
    match classify_prompt_text_secret_key(raw_key) {
        Some(SecretKeyClass::AlwaysRedact) => true,
        Some(SecretKeyClass::ValueAware) => is_value_aware_secret_value(raw_value),
        None => is_secret_bearing_url_value(raw_value),
    }
}

fn contains_secret_label_value_sequence(text: &str) -> bool {
    let tokens = text.split_ascii_whitespace().collect::<Vec<_>>();
    if tokens.len() < 2 {
        return false;
    }

    for label_start in 0..tokens.len() - 1 {
        for label_word_count in 1..=MAX_SECRET_LABEL_WORDS {
            let value_index = label_start + label_word_count;
            if value_index >= tokens.len() {
                break;
            }

            let raw_label = tokens[label_start..value_index].join(" ");
            let normalized_label = normalize_secret_key(&raw_label);
            if let Some(secret_key_class) =
                classify_normalized_prompt_text_secret_key(&normalized_label)
            {
                if whitespace_label_value_is_secret(
                    tokens[value_index],
                    &normalized_label,
                    secret_key_class,
                ) {
                    return true;
                }
            }
        }
    }

    false
}

fn whitespace_label_value_is_secret(
    raw_value: &str,
    normalized_label: &str,
    secret_key_class: SecretKeyClass,
) -> bool {
    let value = trim_value_token(raw_value);
    if value.is_empty() || is_non_secret_label_continuation(value) {
        return false;
    }

    if secret_key_class == SecretKeyClass::ValueAware {
        return is_value_aware_secret_value(value);
    }

    if secret_label_masks_plain_whitespace_value(normalized_label) {
        return true;
    }

    contains_value_only_secret_signal(value) || looks_like_secret_label_value_token(value)
}

fn secret_label_masks_plain_whitespace_value(normalized_label: &str) -> bool {
    ALWAYS_REDACT_SECRET_KEY_NAMES
        .iter()
        .any(|secret_key| normalized_secret_key_matches(normalized_label, secret_key))
}

fn trim_value_token(value: &str) -> &str {
    value.trim_matches(|character| !is_token_value_character(character))
}

fn is_non_secret_label_continuation(value: &str) -> bool {
    let normalized = value.to_ascii_lowercase();

    NON_SECRET_LABEL_CONTINUATION_WORDS.contains(&normalized.as_str())
}

fn looks_like_secret_label_value_token(value: &str) -> bool {
    if value.len() < 6 || !value.chars().all(is_token_value_character) {
        return false;
    }

    let shape = token_shape(value);
    (shape.has_digit && shape.letter_count > 0)
        || shape.has_symbol
        || is_uppercase_secret_label_value(value, shape)
}

fn is_uppercase_secret_label_value(value: &str, shape: TokenShape) -> bool {
    value.len() >= 8
        && shape.has_uppercase
        && !shape.has_lowercase
        && shape.letter_count >= 6
        && value.chars().all(|character| {
            character.is_ascii_uppercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_' | '.' | '~')
        })
}

fn assignment_key_before_separator(text_before_separator: &str) -> &str {
    let trimmed =
        text_before_separator.trim_end_matches(|character: char| character.is_ascii_whitespace());
    let start = trimmed
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            if is_assignment_key_character(character) {
                None
            } else {
                Some(index + character.len_utf8())
            }
        })
        .unwrap_or(0);

    &trimmed[start..]
}

fn is_assignment_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character.is_ascii_whitespace()
        || matches!(character, '_' | '-' | '.')
}

fn classify_prompt_text_secret_key(raw_key: &str) -> Option<SecretKeyClass> {
    let normalized_key = normalize_secret_key(raw_key);
    classify_normalized_prompt_text_secret_key(&normalized_key)
}

fn classify_normalized_prompt_text_secret_key(normalized_key: &str) -> Option<SecretKeyClass> {
    if ALWAYS_REDACT_SECRET_KEY_NAMES
        .iter()
        .any(|secret_key| normalized_secret_key_matches(normalized_key, secret_key))
    {
        Some(SecretKeyClass::AlwaysRedact)
    } else if TEXT_EXACT_VALUE_AWARE_SECRET_KEY_NAMES.contains(&normalized_key) {
        Some(SecretKeyClass::ValueAware)
    } else {
        None
    }
}

fn normalized_secret_key_matches(normalized_key: &str, secret_key: &str) -> bool {
    normalized_key == secret_key
        || normalized_key
            .strip_suffix(secret_key)
            .is_some_and(|prefix| prefix.ends_with('_'))
}

fn normalize_secret_key(raw_key: &str) -> String {
    let mut normalized = String::new();
    let mut last_was_separator = false;
    let mut previous_key_character: Option<char> = None;
    let mut characters = raw_key.trim().chars().peekable();

    while let Some(character) = characters.next() {
        let normalized_character =
            if character.is_ascii_whitespace() || matches!(character, '-' | '.') {
                '_'
            } else {
                character.to_ascii_lowercase()
            };

        if normalized_character == '_' {
            if !last_was_separator {
                normalized.push(normalized_character);
                last_was_separator = true;
            }
            previous_key_character = None;
        } else if character.is_ascii_uppercase()
            && should_start_new_key_word(previous_key_character, characters.peek().copied())
        {
            if !normalized.is_empty() && !last_was_separator {
                normalized.push('_');
            }
            normalized.push(normalized_character);
            last_was_separator = false;
            previous_key_character = Some(character);
        } else {
            normalized.push(normalized_character);
            last_was_separator = false;
            previous_key_character = Some(character);
        }
    }

    normalized
}

fn should_start_new_key_word(previous: Option<char>, next: Option<char>) -> bool {
    match previous {
        Some(previous) if previous.is_ascii_lowercase() => true,
        Some(previous) if previous.is_ascii_uppercase() => {
            next.is_some_and(|next| next.is_ascii_lowercase())
        }
        _ => false,
    }
}

fn is_value_aware_secret_value(raw_value: &str) -> bool {
    url_value_layer_has_secret(raw_value.trim())
}

fn is_numeric_auth_code_token(token: &str) -> bool {
    (6..=8).contains(&token.len()) && token.chars().all(|character| character.is_ascii_digit())
}

fn is_jwt_like_token(token: &str) -> bool {
    let mut segments = token.split('.');

    match (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) {
        (Some(header), Some(payload), Some(signature), None) => {
            token.len() >= 20
                && [header, payload, signature]
                    .iter()
                    .all(|segment| !segment.is_empty() && is_base64url_text(segment))
        }
        _ => false,
    }
}

fn is_high_entropy_token_like_token(token: &str) -> bool {
    token.len() >= 20
        && !is_uuid_like_value(token)
        && !is_human_readable_slug_like(token)
        && !is_human_readable_camel_identifier(token)
        && !is_human_readable_single_word_identifier(token)
        && token.chars().all(is_token_value_character)
        && token_has_high_entropy_shape(token)
}

fn contains_secret_like_value_token(text: &str) -> bool {
    let mut token_start = None;

    for (index, character) in text.char_indices() {
        if is_token_value_character(character) {
            if token_start.is_none() {
                token_start = Some(index);
            }
        } else if let Some(start) = token_start.take() {
            if !token_starts_after_percent_escape(text, start)
                && is_secret_like_value_token(&text[start..index])
            {
                return true;
            }
        }
    }

    token_start.is_some_and(|start| {
        !token_starts_after_percent_escape(text, start)
            && is_secret_like_value_token(&text[start..])
    })
}

fn is_secret_like_value_token(token: &str) -> bool {
    !is_uuid_like_value(token)
        && (is_numeric_auth_code_token(token)
            || is_jwt_like_token(token)
            || is_high_entropy_token_like_token(token))
}

fn token_starts_after_percent_escape(text: &str, start: usize) -> bool {
    start > 0 && text[..start].ends_with('%')
}

fn token_has_high_entropy_shape(token: &str) -> bool {
    let shape = token_shape(token);

    if shape.letter_count == 0 {
        return false;
    }

    if is_hex_like_opaque_token(token, shape) {
        return true;
    }

    if !shape.has_digit {
        return false;
    }

    if shape.has_uppercase && (shape.has_lowercase || shape.has_symbol) {
        return true;
    }

    if shape.has_symbol {
        return true;
    }

    is_single_case_opaque_alphanumeric_token(token, shape)
}

#[derive(Debug, Clone, Copy, Default)]
struct TokenShape {
    has_lowercase: bool,
    has_uppercase: bool,
    has_digit: bool,
    has_symbol: bool,
    digit_count: usize,
    letter_count: usize,
}

fn token_shape(token: &str) -> TokenShape {
    let mut shape = TokenShape::default();

    for character in token.chars() {
        if character.is_ascii_lowercase() {
            shape.has_lowercase = true;
            shape.letter_count += 1;
        } else if character.is_ascii_uppercase() {
            shape.has_uppercase = true;
            shape.letter_count += 1;
        } else if character.is_ascii_digit() {
            shape.has_digit = true;
            shape.digit_count += 1;
        } else if matches!(character, '-' | '_' | '.' | '~') {
            shape.has_symbol = true;
        }
    }

    shape
}

fn is_single_case_opaque_alphanumeric_token(token: &str, shape: TokenShape) -> bool {
    !shape.has_symbol
        && shape.digit_count >= 6
        && (shape.has_lowercase ^ shape.has_uppercase)
        && shape.digit_count * 4 >= token.len()
}

fn is_hex_like_opaque_token(token: &str, shape: TokenShape) -> bool {
    token.len() >= 24
        && !shape.has_symbol
        && token.chars().all(|character| character.is_ascii_hexdigit())
        && token
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}

fn is_human_readable_single_word_identifier(token: &str) -> bool {
    let (letters, trailing_digits) = split_trailing_digits(token);
    if letters.len() < 8 || trailing_digits.is_empty() || trailing_digits.len() > 4 {
        return false;
    }

    letters
        .chars()
        .all(|character| character.is_ascii_lowercase())
        || letters
            .chars()
            .all(|character| character.is_ascii_uppercase())
}

fn is_uuid_like_value(token: &str) -> bool {
    token.len() == 36
        && token.char_indices().all(|(index, character)| match index {
            8 | 13 | 18 | 23 => character == '-',
            _ => character.is_ascii_hexdigit(),
        })
}

fn is_human_readable_slug_like(token: &str) -> bool {
    if !token.contains('-') {
        return false;
    }

    let mut word_segments = 0;
    let all_segments_are_labels = token.split('-').all(|segment| {
        !segment.is_empty()
            && if is_human_word_label(segment) {
                word_segments += 1;
                true
            } else {
                is_short_numeric_label(segment)
            }
    });

    all_segments_are_labels && word_segments >= 2
}

fn is_human_word_label(segment: &str) -> bool {
    segment.len() >= 2
        && (segment
            .chars()
            .all(|character| character.is_ascii_lowercase())
            || segment
                .chars()
                .all(|character| character.is_ascii_uppercase())
            || is_titlecase_word_label(segment))
}

fn is_titlecase_word_label(segment: &str) -> bool {
    let mut characters = segment.chars();
    matches!(characters.next(), Some(first) if first.is_ascii_uppercase())
        && characters.all(|character| character.is_ascii_lowercase())
}

fn is_short_numeric_label(segment: &str) -> bool {
    (2..=4).contains(&segment.len()) && segment.chars().all(|character| character.is_ascii_digit())
}

fn is_human_readable_camel_identifier(token: &str) -> bool {
    let (letters, trailing_digits) = split_trailing_digits(token);
    if letters.len() == token.len() || trailing_digits.len() > 4 {
        return false;
    }
    if !letters
        .chars()
        .all(|character| character.is_ascii_alphabetic())
    {
        return false;
    }

    let words = camel_identifier_words(letters);
    words.len() >= 2 && words.iter().all(|word| word.len() >= 3)
}

fn split_trailing_digits(value: &str) -> (&str, &str) {
    let digit_start = value
        .char_indices()
        .rev()
        .find_map(|(index, character)| {
            if character.is_ascii_digit() {
                None
            } else {
                Some(index + character.len_utf8())
            }
        })
        .unwrap_or(0);

    value.split_at(digit_start)
}

fn camel_identifier_words(identifier: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut start = 0;

    for (index, character) in identifier.char_indices().skip(1) {
        if character.is_ascii_uppercase() {
            words.push(&identifier[start..index]);
            start = index;
        }
    }

    words.push(&identifier[start..]);
    words
}

fn is_base64url_text(text: &str) -> bool {
    text.chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

fn is_token_value_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '~')
}

fn contains_card_like_digit_sequence(text: &str) -> bool {
    let characters = text.char_indices().collect::<Vec<_>>();
    let mut index = 0;

    while index < characters.len() {
        let (byte_index, character) = characters[index];
        if !character.is_ascii_digit() {
            index += 1;
            continue;
        }

        let mut digits = 0;
        let mut cursor = index;
        while cursor < characters.len() {
            let character = characters[cursor].1;
            if character.is_ascii_digit() {
                digits += 1;
                cursor += 1;
            } else if matches!(character, ' ' | '-') && digits > 0 {
                cursor += 1;
            } else {
                break;
            }
        }

        if (13..=19).contains(&digits) && !is_inside_uuid_like_token(text, byte_index) {
            return true;
        }

        index = cursor.max(index + 1);
    }

    false
}

fn is_inside_uuid_like_token(text: &str, byte_index: usize) -> bool {
    is_uuid_like_value(ascii_alphanumeric_hyphen_token_at(text, byte_index))
}

fn ascii_alphanumeric_hyphen_token_at(text: &str, byte_index: usize) -> &str {
    let mut start = byte_index;
    while start > 0 {
        let Some(previous) = text[..start].chars().next_back() else {
            break;
        };
        if !is_ascii_alphanumeric_hyphen(previous) {
            break;
        }
        start -= previous.len_utf8();
    }

    let mut end = byte_index;
    while end < text.len() {
        let Some(next) = text[end..].chars().next() else {
            break;
        };
        if !is_ascii_alphanumeric_hyphen(next) {
            break;
        }
        end += next.len_utf8();
    }

    &text[start..end]
}

fn is_ascii_alphanumeric_hyphen(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-'
}

const SECRET_TEXT_SIGNALS: &[&str] = &["authorization:", "bearer ", "sk-"];

const ALWAYS_REDACT_SECRET_KEY_NAMES: &[&str] = &[
    "token",
    "secret",
    "api_key",
    "apikey",
    "x_api_key",
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "password",
    "passcode",
    "reset_code",
    "verification_code",
    "otp",
    "2fa",
];

const TEXT_EXACT_VALUE_AWARE_SECRET_KEY_NAMES: &[&str] = &["code"];

const URL_EXACT_ALWAYS_REDACT_SECRET_KEY_NAMES: &[&str] = &["code"];

const NON_SECRET_LABEL_CONTINUATION_WORDS: &[&str] = &[
    "button",
    "bucket",
    "budget",
    "change",
    "count",
    "documentation",
    "field",
    "form",
    "help",
    "input",
    "label",
    "limit",
    "manager",
    "now",
    "page",
    "placeholder",
    "policy",
    "required",
    "requirement",
    "requirements",
    "reset",
    "rule",
    "rules",
    "setting",
    "settings",
    "strength",
    "update",
    "usage",
    "value",
    "values",
];

const MAX_SECRET_LABEL_WORDS: usize = 3;

const MAX_URL_VALUE_DECODE_DEPTH: usize = 5;
