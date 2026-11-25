//! Presentation: `playpilot mail send | inbox | read | ack`.

use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use super::Backend;
use super::message::{Envelope, Stored};
use super::transport::Receipt;

pub const USAGE: &str = "\
usage: playpilot mail send  --to <actor> --subject <text> [--re <ref>] (--body <text> | --body-file <path|->)
       playpilot mail inbox [--all] [--days <n>]
       playpilot mail read  <message-id>
       playpilot mail ack   <message-id>
common: [--repo <owner/name>] (or PLAYPILOT_MAILBOX_REPO) [--as <actor>] (or PLAYPILOT_ACTOR)
mailbox: the Worker at PLAYPILOT_MAIL_URL (token from PLAYPILOT_MCP_TOKEN, if set) when set,
         otherwise the GitHub repository";

struct Args {
    verb: String,
    repo: Option<String>,
    actor: Option<String>,
    to: Option<String>,
    subject: Option<String>,
    re: Option<String>,
    body: Option<String>,
    body_file: Option<String>,
    all: bool,
    days: u64,
    id: Option<u64>,
}

fn parse(args: Vec<String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let verb = it.next().ok_or("missing verb")?;
    let mut a = Args {
        verb,
        repo: std::env::var("PLAYPILOT_MAILBOX_REPO").ok(),
        actor: std::env::var("PLAYPILOT_ACTOR").ok(),
        to: None,
        subject: None,
        re: None,
        body: None,
        body_file: None,
        all: false,
        days: 7,
        id: None,
    };
    while let Some(x) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match x.as_str() {
            "--repo" => a.repo = Some(value("--repo")?),
            "--as" | "--from" => a.actor = Some(value("--as")?),
            "--to" => a.to = Some(value("--to")?),
            "--subject" => a.subject = Some(value("--subject")?),
            "--re" => a.re = Some(value("--re")?),
            "--body" => a.body = Some(value("--body")?),
            "--body-file" => a.body_file = Some(value("--body-file")?),
            "--days" => {
                a.days = value("--days")?
                    .parse()
                    .map_err(|_| "--days needs a number".to_string())?
            }
            "--all" => a.all = true,
            id if a.id.is_none() && id.chars().all(|c| c.is_ascii_digit()) => {
                a.id = id.parse().ok()
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn read_body(a: &Args) -> Result<String, String> {
    match (&a.body, &a.body_file) {
        (Some(b), None) => Ok(b.clone()),
        (None, Some(p)) if p == "-" => {
            let mut s = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut s)
                .map_err(|e| format!("could not read stdin: {e}"))?;
            Ok(s)
        }
        (None, Some(p)) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}")),
        _ => Err("give exactly one of --body or --body-file".into()),
    }
}

/// One message as `inbox` and `read` print it: a header line, then the
/// body indented by four spaces.
pub fn render(m: &Stored) -> String {
    let e = &m.envelope;
    let state = match (m.read, m.acked) {
        (_, true) => "acked",
        (true, false) => "read",
        _ => "new",
    };
    let mut out = format!(
        "{}  {}  {} -> {}  [{state}]  {}{}\n",
        m.id,
        m.created_at,
        e.from,
        e.to,
        e.subject,
        e.re.as_deref()
            .map(|r| format!("  (re {r})"))
            .unwrap_or_default()
    );
    for line in e.body.lines() {
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn run(args: Vec<String>) -> Result<(), String> {
    let a = parse(args)?;
    let backend = Backend::from_env(a.repo.clone())
        .ok_or("set PLAYPILOT_MAIL_URL, or --repo / PLAYPILOT_MAILBOX_REPO")?;
    let transport = backend.open(a.actor.clone());
    let t = transport.as_ref();
    let actor = || a.actor.clone().ok_or("set --as or PLAYPILOT_ACTOR");
    match a.verb.as_str() {
        "send" => {
            let envelope = Envelope {
                from: actor()?,
                to: a.to.clone().ok_or("--to is required")?,
                subject: a.subject.clone().ok_or("--subject is required")?,
                re: a.re.clone(),
                body: read_body(&a)?,
            };
            for (mailbox, id) in super::send(t, envelope)? {
                println!("sent {id} to {mailbox}");
            }
        }
        "inbox" => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let since = super::days_ago(now, a.days);
            let messages = super::inbox(t, &actor()?, a.all, &since)?;
            if messages.is_empty() {
                println!("inbox: nothing new");
            }
            for m in messages {
                print!("{}", render(&m));
            }
        }
        "read" => {
            let id = a.id.ok_or("give the message id")?;
            print!("{}", render(&super::read(t, &actor()?, id)?));
        }
        "ack" => {
            let id = a.id.ok_or("give the message id")?;
            super::mark(t, id, Receipt::Ack)?;
            println!("ack {id}");
        }
        other => return Err(format!("unknown mail verb `{other}`")),
    }
    Ok(())
}

pub fn main(args: Vec<String>) -> ExitCode {
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        eprintln!("{USAGE}");
        return ExitCode::from(64);
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("playpilot mail: {e}");
            ExitCode::from(1)
        }
    }
}
