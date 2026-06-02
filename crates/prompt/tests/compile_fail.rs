#[test]
fn prompt_api_rejects_raw_or_unchecked_safe_text() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/compile_fail/*.rs");
}
