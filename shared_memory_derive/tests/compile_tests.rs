#[test]
fn compile_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/run-pass/*.rs");
    t.compile_fail("tests/ui/*.rs");
}
