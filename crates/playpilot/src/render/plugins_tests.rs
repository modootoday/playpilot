use std::path::Path;

use serde_json::Value;

use super::*;
use crate::validate::{parse, read_dir};

const INDEX_SCHEMA: &str = include_str!("../../../../schemas/plugin-index.v1.schema.json");
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn plugin(name: &str, spec: &str) -> Resource {
    let text = format!(
        "apiVersion = \"playpilot.io/v1\"\nkind = \"Plugin\"\n[metadata]\nname = \"{name}\"\n[spec]\n{spec}\n"
    );
    parse(&format!("{name}.toml"), &text).unwrap()
}

fn good(name: &str, interface: &str, extra: &str) -> Resource {
    plugin(
        name,
        &format!(
            "interface = \"playpilot:fleet/{interface}@1.0.0\"\n\
             source = {{ url = \"https://example.invalid/{name}.wasm\", sha256 = \"{SHA}\" }}\n{extra}"
        ),
    )
}

fn refused(spec: &str) -> String {
    entries(&[plugin("bad", spec)]).unwrap_err()
}

/// Checks `v` against the parts of JSON Schema the index schema uses:
/// `$ref` (local), `type`, `const`, `enum`, `required`, `properties`,
/// `additionalProperties: false` and `items`. Patterns are checked in Rust.
fn conforms(root: &Value, schema: &Value, v: &Value, at: &str, out: &mut Vec<String>) {
    if let Some(r) = schema["$ref"].as_str() {
        let name = r.strip_prefix("#/$defs/").expect("local $ref");
        return conforms(root, &root["$defs"][name], v, at, out);
    }
    if let Some(t) = schema["type"].as_str() {
        let ok = match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            other => panic!("type {other} not handled"),
        };
        if !ok {
            out.push(format!("{at}: not a {t}"));
            return;
        }
    }
    if let Some(c) = schema.get("const")
        && c != v
    {
        out.push(format!("{at}: must be {c}"));
    }
    if let Some(e) = schema["enum"].as_array()
        && !e.contains(v)
    {
        out.push(format!("{at}: {v} not in enum"));
    }
    if let Some(obj) = v.as_object() {
        for k in schema["required"].as_array().into_iter().flatten() {
            if !obj.contains_key(k.as_str().unwrap()) {
                out.push(format!("{at}: missing {k}"));
            }
        }
        for (k, x) in obj {
            match schema["properties"].get(k) {
                Some(s) => conforms(root, s, x, &format!("{at}.{k}"), out),
                None if schema["additionalProperties"] == false => {
                    out.push(format!("{at}: unexpected {k}"));
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(a)) = (schema.get("items"), v.as_array()) {
        for (i, x) in a.iter().enumerate() {
            conforms(root, items, x, &format!("{at}[{i}]"), out);
        }
    }
}

fn schema_problems(index: &Value) -> Vec<String> {
    let schema: Value = serde_json::from_str(INDEX_SCHEMA).expect("schema is JSON");
    let mut out = Vec::new();
    conforms(&schema, &schema, index, "$", &mut out);
    out
}

#[test]
fn example_fleet_index_validates_against_the_schema() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fleet");
    let resources = read_dir(&dir).expect("example fleet parses");
    assert!(crate::validate::check(&resources).is_empty());
    let text = index_text(&resources).expect("example plugins index");
    assert!(text.ends_with("}\n"));
    let index: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(schema_problems(&index), Vec::<String>::new());
    let plugins = index["plugins"].as_array().unwrap();
    assert!(!plugins.is_empty(), "the example fleet declares a plugin");
    for p in plugins {
        assert!(is_sha256(p["source"]["sha256"].as_str().unwrap()));
        assert!(p["source"]["url"].as_str().unwrap().starts_with("https://"));
    }
}

#[test]
fn schema_lists_the_interfaces_and_kinds_the_code_accepts() {
    let s: Value = serde_json::from_str(INDEX_SCHEMA).unwrap();
    let names = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect()
    };
    let p = &s["$defs"]["plugin"]["properties"];
    assert_eq!(names(&p["interface"]["enum"]), INTERFACES);
    assert_eq!(names(&p["kind"]["enum"]), KINDS);
    assert_eq!(s["properties"]["version"]["const"], INDEX_VERSION);
}

#[test]
fn the_checker_catches_what_the_schema_forbids() {
    let mut bad = index(&entries(&[good("a", "runtime", "")]).unwrap());
    bad["plugins"][0]["interface"] = "shell".into();
    bad["plugins"][0].as_object_mut().unwrap().remove("source");
    bad["extra"] = true.into();
    let found = schema_problems(&bad);
    assert_eq!(found.len(), 3, "{found:?}");
}

#[test]
fn entries_are_sorted_with_sorted_grants_and_render_deterministically() {
    let fleet = vec![
        good(
            "zeta-sink",
            "event-sink",
            "kind = \"process\"\ngrants = { network = [\"hooks.example.invalid\", \"api.example.invalid\"], secrets = [{ secretRef = \"SINK_KEY\" }, \"AN_ID\"] }",
        ),
        good("alpha-policy", "policy-check", ""),
    ];
    let text = index_text(&fleet).unwrap();
    let mut reversed = fleet.clone();
    reversed.reverse();
    assert_eq!(index_text(&reversed).unwrap(), text);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["plugins"][0]["id"], "alpha-policy");
    assert_eq!(v["plugins"][0]["kind"], "wasm-component");
    assert_eq!(v["plugins"][0]["grants"]["network"], serde_json::json!([]));
    let z = &v["plugins"][1];
    assert_eq!(z["interface"], "event-sink");
    assert_eq!(z["kind"], "process");
    assert_eq!(
        z["grants"]["network"],
        serde_json::json!(["api.example.invalid", "hooks.example.invalid"])
    );
    assert_eq!(
        z["grants"]["secrets"],
        serde_json::json!(["AN_ID", "SINK_KEY"])
    );
    assert_eq!(schema_problems(&v), Vec::<String>::new());
}

#[test]
fn an_empty_fleet_has_an_empty_index() {
    assert_eq!(
        index_text(&[]).unwrap(),
        "{\n  \"version\": 1,\n  \"plugins\": []\n}\n"
    );
}

#[test]
fn refuses_bad_pins() {
    let iface = "interface = \"playpilot:fleet/runtime@1.0.0\"\n";
    for (source, why) in [
        (
            format!("source = {{ url = \"http://example.invalid/p.wasm\", sha256 = \"{SHA}\" }}"),
            "https",
        ),
        (
            format!("source = {{ url = \"https:///p.wasm\", sha256 = \"{SHA}\" }}"),
            "https",
        ),
        (
            format!("source = {{ url = \"https://a b/p\", sha256 = \"{SHA}\" }}"),
            "https",
        ),
        (
            "source = { url = \"https://example.invalid/p.wasm\", sha256 = \"<pinned digest>\" }"
                .into(),
            "64",
        ),
        (
            format!(
                "source = {{ url = \"https://example.invalid/p.wasm\", sha256 = \"{}\" }}",
                SHA.to_uppercase()
            ),
            "64",
        ),
        (
            format!(
                "source = {{ url = \"https://example.invalid/p.wasm\", sha256 = \"{}\" }}",
                &SHA[1..]
            ),
            "64",
        ),
        (
            "source = { url = \"https://example.invalid/p.wasm\" }".into(),
            "sha256 is required",
        ),
        (String::new(), "spec.source"),
    ] {
        let e = refused(&format!("{iface}{source}"));
        assert!(e.contains(why), "{source}: {e}");
    }
}

#[test]
fn refuses_unknown_interfaces_and_kinds() {
    for wit in [
        "playpilot:fleet/shell@1.0.0",
        "playpilot:fleet/runtime@2.0.0",
        "playpilot:fleet/runtime",
        "other:pkg/runtime@1.0.0",
        "runtime",
    ] {
        let e = refused(&format!(
            "interface = \"{wit}\"\nsource = {{ url = \"https://example.invalid/p\", sha256 = \"{SHA}\" }}"
        ));
        assert!(e.contains("is not one of"), "{wit}: {e}");
    }
    let e = entries(&[good("p", "runtime", "kind = \"docker\"")]).unwrap_err();
    assert!(e.contains("spec.kind"), "{e}");
}

#[test]
fn refuses_secret_values_and_unknown_grants() {
    for (grants, why) in [
        (
            "grants = { secrets = [\"sk-live-abc123\"] }",
            "never a value",
        ),
        ("grants = { secrets = [{ value = \"abc\" }] }", "secretRef"),
        (
            "grants = { secrets = [{ secretRef = \"A\", value = \"abc\" }] }",
            "secretRef",
        ),
        ("grants = { secrets = [\"A\", \"A\"] }", "twice"),
        (
            "grants = { network = [\"https://api.example.invalid\"] }",
            "host name",
        ),
        ("grants = { filesystem = [\"/\"] }", "not a known grant"),
        ("apiToken = \"abc\"", "literal value"),
        ("[spec.extensions.x-p]\npassword = \"abc\"", "literal value"),
    ] {
        let e = entries(&[good("p", "runtime", grants)]).unwrap_err();
        assert!(e.contains(why), "{grants}: {e}");
        assert!(
            !e.contains("sk-live-abc123"),
            "the value is never echoed: {e}"
        );
    }
}

#[test]
fn every_problem_is_reported_with_its_file() {
    let e = entries(&[
        good("one", "shell", ""),
        good("two", "runtime", ""),
        good("two", "runtime", ""),
    ])
    .unwrap_err();
    assert!(e.contains("one.toml: spec.interface"), "{e}");
    assert!(e.contains("two.toml: duplicate Plugin \"two\""), "{e}");
}
