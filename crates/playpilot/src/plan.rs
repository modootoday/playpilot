//! `playpilot plan` and `playpilot whoami`: who does what, read from the
//! fleet configuration (device × lane × project × role), and what nobody covers.

use std::collections::BTreeSet;

use crate::validate::{Resource, strings};

fn of<'a>(resources: &'a [Resource], kind: &'a str) -> impl Iterator<Item = &'a Resource> {
    resources.iter().filter(move |r| r.kind == kind)
}

fn caps(device: &Resource) -> BTreeSet<String> {
    strings(&device.spec, "capabilities").into_iter().collect()
}

fn can_serve(device: &Resource, lane: &Resource) -> bool {
    let req: BTreeSet<String> = strings(&lane.spec, "requires").into_iter().collect();
    req.is_subset(&caps(device))
}

fn text(r: &Resource, key: &str) -> String {
    r.spec
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or("-")
        .to_string()
}

fn schedule(session: &Resource) -> String {
    session
        .spec
        .get("trigger")
        .and_then(|t| t.get("schedule"))
        .and_then(toml::Value::as_str)
        .unwrap_or("on demand")
        .to_string()
}

/// The whole fleet: lanes with their primary and fallback devices and the
/// projects that use them, sessions, and gaps.
pub fn plan(resources: &[Resource]) -> String {
    let mut out = String::from("Lanes\n");
    let mut gaps = Vec::new();
    for lane in of(resources, "Lane") {
        let projects: Vec<&str> = of(resources, "Project")
            .filter(|p| strings(&p.spec, "lanes").contains(&lane.name))
            .map(|p| p.name.as_str())
            .collect();
        let (mut primary, mut fallback) = (Vec::new(), Vec::new());
        for d in of(resources, "Device").filter(|d| can_serve(d, lane)) {
            if strings(&d.spec, "primaryLanes").contains(&lane.name) {
                primary.push(d.name.as_str());
            } else {
                fallback.push(d.name.as_str());
            }
        }
        out.push_str(&format!(
            "  {} [{}]: primary {}; fallback {}; projects {}\n",
            lane.name,
            text(lane, "label"),
            list(&primary),
            list(&fallback),
            list(&projects)
        ));
        if !projects.is_empty() && primary.is_empty() && fallback.is_empty() {
            gaps.push(format!(
                "lane {} is used by {} but no device can serve it",
                lane.name,
                projects.join(", ")
            ));
        } else if !projects.is_empty() && primary.is_empty() {
            gaps.push(format!("lane {} has only fallback devices", lane.name));
        }
    }
    out.push_str("Sessions\n");
    for s in of(resources, "Session") {
        out.push_str(&format!(
            "  {}: {} on {}, role {}, {}\n",
            s.name,
            text(s, "project"),
            text(s, "runtime"),
            text(s, "role"),
            schedule(s)
        ));
    }
    for p in of(resources, "Project") {
        let has_session = of(resources, "Session").any(|s| text(s, "project") == p.name);
        if !has_session {
            gaps.push(format!("project {} has no session", p.name));
        }
    }
    out.push_str("Gaps\n");
    if gaps.is_empty() {
        out.push_str("  none\n");
    }
    for g in gaps {
        out.push_str(&format!("  {g}\n"));
    }
    out
}

/// One device's view: its roles, lanes (primary and fallback) and the
/// projects those lanes serve.
pub fn whoami(resources: &[Resource], device: &str) -> Result<String, String> {
    let d = of(resources, "Device")
        .find(|r| r.name == device)
        .ok_or_else(|| format!("no Device \"{device}\" in the fleet configuration"))?;
    let primary = strings(&d.spec, "primaryLanes");
    let mut out = format!("device {}\n", d.name);
    out.push_str(&format!("  claim id: {}\n", text(d, "claimId")));
    out.push_str("  roles:\n");
    for role in strings(&d.spec, "roles") {
        let agent = of(resources, "Role")
            .find(|r| r.name == role)
            .map(|r| text(r, "agent"))
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!("    {role} ({agent})\n"));
    }
    out.push_str("  lanes:\n");
    for lane in of(resources, "Lane").filter(|l| can_serve(d, l)) {
        let kind = if primary.contains(&lane.name) {
            "primary"
        } else {
            "fallback"
        };
        let projects: Vec<&str> = of(resources, "Project")
            .filter(|p| strings(&p.spec, "lanes").contains(&lane.name))
            .map(|p| p.name.as_str())
            .collect();
        out.push_str(&format!(
            "    {} [{}] {kind}: {}\n",
            lane.name,
            text(lane, "label"),
            list(&projects)
        ));
    }
    Ok(out)
}

fn list(v: &[&str]) -> String {
    if v.is_empty() {
        "none".into()
    } else {
        v.join(", ")
    }
}
