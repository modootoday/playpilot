//! `POST /mcp`: runs MCP tool commands against the Durable Objects as the
//! caller's mapped actor and claim id.

use worker::{Env, Request, Response, Result};

use super::{call, http, settings};
use crate::domain::bus::message::{Envelope, Receipt};
use crate::domain::bus::{BusCmd, BusReply};
use crate::domain::identity::Identity;
use crate::domain::lease::{LeaseCmd, LeaseReply, Limits, clamp_ttl};
use crate::domain::mcp::protocol::{Step, handle as parse, tool_result};
use crate::domain::mcp::tools::{self, Command};

pub async fn handle(mut req: Request, env: &Env, caller: &Identity) -> Result<Response> {
    match parse(&req.text().await?) {
        Step::Reply(v) => http::json(200, &v),
        Step::Accepted => http::empty(202),
        Step::Call { id, command } => {
            http::json(200, &tool_result(&id, run(env, caller, command).await))
        }
    }
}

async fn bus(env: &Env, cmd: BusCmd) -> std::result::Result<BusReply, String> {
    match call::bus(env, &cmd).await? {
        BusReply::Error(e) => Err(e),
        ok => Ok(ok),
    }
}

async fn run(
    env: &Env,
    caller: &Identity,
    command: Command,
) -> std::result::Result<String, String> {
    let actor = caller.actor.clone();
    match command {
        Command::Whoami => Ok(tools::render_whoami(&caller.actor, &caller.claim)),
        Command::MailInbox { all } => match bus(env, BusCmd::Inbox { actor, all }).await? {
            BusReply::Messages(m) => Ok(tools::render_inbox(&m)),
            other => Err(format!("unexpected reply {other:?}")),
        },
        Command::MailSend {
            to,
            subject,
            body,
            re,
        } => {
            let directory = settings::identities(env)?.actors();
            let envelope = Envelope {
                from: actor,
                to,
                subject,
                re,
                body,
            };
            match bus(
                env,
                BusCmd::Send {
                    envelope,
                    directory,
                },
            )
            .await?
            {
                BusReply::Sent(s) => Ok(tools::render_sent(&s)),
                other => Err(format!("unexpected reply {other:?}")),
            }
        }
        Command::MailRead { id } | Command::MailAck { id } => {
            let (kind, verb) = if matches!(command, Command::MailRead { .. }) {
                (Receipt::Read, "read")
            } else {
                (Receipt::Ack, "ack")
            };
            bus(env, BusCmd::Mark { actor, id, kind }).await?;
            Ok(format!("{verb} {id}"))
        }
        Command::TaskClaim { item, lane, ttl } => {
            let ttl = clamp_ttl(ttl).map_err(|e| e.detail())?;
            let switches = settings::switches(env)?;
            let limits = Limits {
                lane: switches.effective("Lane", &lane),
                device: switches.effective("Device", &caller.claim),
            };
            let cmd = LeaseCmd::Acquire {
                lane,
                item: item.clone(),
                holder: caller.claim.clone(),
                ttl,
                limits,
            };
            match call::leases(env, &cmd).await? {
                LeaseReply::Lease(l) => tools::render_claim(&item, Ok(l)),
                LeaseReply::Error(e) => tools::render_claim(&item, Err(e)),
                other => Err(format!("unexpected reply {other:?}")),
            }
        }
        Command::TaskRelease { item } => {
            let cmd = LeaseCmd::ReleaseItem {
                item: item.clone(),
                holder: caller.claim.clone(),
            };
            match call::leases(env, &cmd).await? {
                LeaseReply::Lease(_) => Ok(format!("released {}#{}", item.repo, item.number)),
                LeaseReply::Error(e) => Err(e.detail()),
                other => Err(format!("unexpected reply {other:?}")),
            }
        }
    }
}
