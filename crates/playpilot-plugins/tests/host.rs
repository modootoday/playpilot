mod support;

use playpilot_plugins::{
    Entry, Event, Host, Interface, Limits, check_all, check_env, deliver_all, parse_policy_env,
};
use support::*;

fn host() -> Host {
    Host::new(Limits::default()).expect("host")
}

fn event(kind: &str) -> Event {
    Event {
        id: "evt-1".into(),
        type_: kind.into(),
        source: "https://example.com/fleet".into(),
        time: "2026-11-24T00:00:00Z".into(),
        data_json: "{}".into(),
    }
}

#[test]
fn policy_allows_an_ordinary_command() {
    let cache = cache_dir();
    let host = host();
    let p = host
        .load_policy(&policy(&cache, "deny-rm", POLICY_DENY_RM))
        .unwrap();
    assert_eq!(p.check("Bash", "cargo test --locked"), Ok(()));
}

#[test]
fn policy_refuses_its_pattern() {
    let cache = cache_dir();
    let host = host();
    let p = host
        .load_policy(&policy(&cache, "deny-rm", POLICY_DENY_RM))
        .unwrap();
    let err = p.check("Bash", "cd /work && rm -rf build").unwrap_err();
    assert!(err.contains("deny-rm refused"), "{err}");
    assert!(err.contains("rm -rf is not allowed"), "{err}");
}

#[test]
fn trapping_policy_refuses() {
    let cache = cache_dir();
    let host = host();
    let p = host
        .load_policy(&policy(&cache, "trap", POLICY_TRAP))
        .unwrap();
    let err = p.check("Bash", "ls").unwrap_err();
    assert!(err.contains("failed, refusing"), "{err}");
}

#[test]
fn policy_out_of_fuel_refuses() {
    let cache = cache_dir();
    let host = Host::new(Limits {
        fuel: 1_000_000,
        ..Limits::default()
    })
    .unwrap();
    let p = host
        .load_policy(&policy(&cache, "spin", POLICY_SPIN))
        .unwrap();
    assert!(p.check("Bash", "ls").is_err());
}

#[test]
fn policy_over_the_memory_cap_refuses() {
    let cache = cache_dir();
    let host = host();
    let p = host
        .load_policy(&policy(&cache, "big", POLICY_BIG_MEMORY))
        .unwrap();
    assert!(p.check("Bash", "ls").is_err());
}

#[test]
fn sha_mismatch_refuses_before_loading() {
    let cache = cache_dir();
    let mut e = policy(&cache, "deny-rm", POLICY_DENY_RM);
    e.sha256 = "0".repeat(64);
    let err = host().load_policy(&e).err().unwrap();
    assert!(err.contains("SHA-256 mismatch"), "{err}");
}

#[test]
fn missing_file_and_wrong_interface_refuse() {
    let cache = cache_dir();
    let host = host();
    let mut e = policy(&cache, "deny-rm", POLICY_DENY_RM);
    let sink_entry = Entry {
        interface: Interface::EventSink,
        ..e.clone()
    };
    assert!(host.load_policy(&sink_entry).is_err());
    e.path = cache.join("absent.wasm");
    assert!(host.load_policy(&e).is_err());
}

#[test]
fn grants_are_refused() {
    let cache = cache_dir();
    let host = host();
    let mut p = policy(&cache, "deny-rm", POLICY_DENY_RM);
    p.network = vec!["example.com".into()];
    assert!(host.load_policy(&p).err().unwrap().contains("network"));
    let mut s = sink(&cache, "echo", SINK_ECHO);
    s.network = vec!["example.com".into()];
    assert!(host.load_sink(&s).err().unwrap().contains("network"));
    s.network.clear();
    s.secrets = vec!["SLACK_TOKEN".into()];
    assert!(host.load_sink(&s).err().unwrap().contains("secret"));
}

#[test]
fn check_all_needs_every_plugin_to_allow() {
    let cache = cache_dir();
    let host = host();
    let allow = host
        .load_policy(&policy(&cache, "deny-rm", POLICY_DENY_RM))
        .unwrap();
    let trap = host
        .load_policy(&policy(&cache, "trap", POLICY_TRAP))
        .unwrap();
    assert!(check_all(std::slice::from_ref(&allow), "Bash", "ls").is_ok());
    assert!(check_all(&[allow, trap], "Bash", "ls").is_err());
    assert!(check_all(&[], "Bash", "ls").is_ok());
}

#[test]
fn sink_receives_the_event() {
    let cache = cache_dir();
    let host = host();
    let s = host.load_sink(&sink(&cache, "echo", SINK_ECHO)).unwrap();
    assert_eq!(s.deliver(&event("com.example.ok")), Ok(()));
    let err = s.deliver(&event("com.example.other")).unwrap_err();
    assert!(err.contains("reported: com.example.other"), "{err}");
}

#[test]
fn failing_sinks_do_not_fail_the_caller() {
    let cache = cache_dir();
    let host = host();
    let trap = host.load_sink(&sink(&cache, "trap", SINK_TRAP)).unwrap();
    let echo = host.load_sink(&sink(&cache, "echo", SINK_ECHO)).unwrap();
    let failures = deliver_all(&[trap, echo], &event("com.example.ok"));
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("event sink trap failed"));
}

#[test]
fn env_value_is_parsed_strictly() {
    let sha = "a".repeat(64);
    let list = parse_policy_env(&format!(" {sha}=one.wasm ; {sha}=two.wasm;")).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[1].path, std::path::PathBuf::from("two.wasm"));
    for bad in ["", ";", "one.wasm", "abc=one.wasm", &format!("{sha}=")] {
        assert!(parse_policy_env(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn check_env_runs_the_listed_plugins() {
    let cache = cache_dir();
    let e = policy(&cache, "deny-rm", POLICY_DENY_RM);
    let value = format!("{}={}", e.sha256, e.path.display());
    assert!(check_env(&value, "Bash", "git status").is_ok());
    assert!(check_env(&value, "Bash", "rm -rf /").is_err());
    let wrong = format!("{}={}", "0".repeat(64), e.path.display());
    assert!(check_env(&wrong, "Bash", "git status").is_err());
}

#[test]
fn index_entry_resolves_to_the_cache() {
    let cache = cache_dir();
    let sha = "b".repeat(64);
    let v = serde_json::json!({
        "id": "deny-rm",
        "kind": "wasm-component",
        "interface": "policy-check",
        "wit": "playpilot:fleet/policy-check@1.0.0",
        "source": { "url": "https://plugins.example.com/deny-rm.wasm", "sha256": sha },
        "grants": { "network": [], "secrets": [] }
    });
    let e = Entry::from_index(&v, &cache).unwrap();
    assert_eq!(e.interface, Interface::PolicyCheck);
    assert_eq!(e.path, cache.join(format!("{sha}.wasm")));
    let mut runtime = v.clone();
    runtime["interface"] = "runtime".into();
    assert!(Entry::from_index(&runtime, &cache).is_err());
    let mut process = v.clone();
    process["kind"] = "process".into();
    assert!(Entry::from_index(&process, &cache).is_err());
}
