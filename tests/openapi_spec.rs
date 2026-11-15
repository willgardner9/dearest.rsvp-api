use dearest::openapi::spec;

/// The committed `openapi.json` must match the spec that the code builds.
/// The test compares parsed values, so the order of keys does not matter.
#[test]
fn committed_spec_is_current() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.json");
    let text = std::fs::read_to_string(path)
        .expect("Cannot read openapi.json. Run `cargo run --bin openapi > openapi.json`.");
    let committed: serde_json::Value =
        serde_json::from_str(&text).expect("openapi.json is not valid JSON.");
    let current = serde_json::to_value(spec()).expect("The spec must convert to JSON.");
    assert_eq!(
        committed, current,
        "openapi.json is out of date. Run `cargo run --bin openapi > openapi.json`."
    );
}
