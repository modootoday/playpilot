//! `POST /mcp`: runs MCP tool commands against the Durable Objects as the
//! actor and claim id the caller's credential allows (the `as` argument picks
//! one when it allows several).

use serde_json::Value;
use worker::{Env, Request, Response, Result};

use super::{call, http, settings, wake};
use crate::domain::bus::message::{Envelope, Receipt};
use crate::domain::bus::{BusCmd, BusReply};
use crate::domain::eventlog::LogReply;
use crate::domain::lease::{LeaseCmd, LeaseReply, Limits, clamp_ttl};
use crate::domain::mcp::events::{self, EventsRead};
use crate::domain::mcp::protocol::{Step, handle as parse, tool_result, tool_result_structured};
use crate::domain::mcp::tools::{self, Command, Invocation};
use crate::domain::token::Caller;

pub async fn handle(mut req: Request, env: &Env, caller: &Caller) -> Result<Response> {
    match parse(&req.text().await?) {
        Step::Reply(v) => http::json(200, &v),
        Step::Accepted => http::empty(202),
        Step::Call { id, call } => http::json(200, &run(env, caller, &id, call).await),
    }
}

async fn bus(env: &Env, cmd: BusCmd) -> std::result::Result<BusReply, String> {
    match call::bus(env, &cmd).await? {
        BusReply::Error(e) => Err(e),
        ok => Ok(ok),
    }
}

async fn run(env: &Env, caller: &Caller, id: &Value, call: Invocation) -> Value {
    if call.command == Command::Whoami {
        let actors = caller.actors();
        let current = caller.resolve(call.as_actor.as_deref()).ok();
        let current = current
            .as_ref()
            .map(|c| (c.actor.as_str(), c.claim.as_str()));
        return tool_result(id, Ok(tools::render_whoami(&actors, current)));
    }
    if let Command::MailInbox { all } = call.command {
        let outcome = match caller.resolve(call.as_actor.as_deref()) {
            Ok(me) => {
                bus(
                    env,
                    BusCmd::Inbox {
                        actor: me.actor,
                        all,
                    },
                )
                .await
            }
            Err(e) => Err(e),
        };
        return match outcome {
            Ok(BusReply::Messages(m)) => {
                tool_result_structured(id, tools::render_inbox(&m), tools::inbox_structured(&m))
            }
            Ok(other) => tool_result(id, Err(format!("unexpected reply {other:?}"))),
            Err(e) => tool_result(id, Err(e)),
        };
    }
    if let Command::EventsRead(read) = &call.command {
        return read_events(env, caller, id, read, call.as_actor.as_deref()).await;
    }
    tool_result(id, run_text(env, caller, call).await)
}

async fn read_events(
    env: &Env,
    caller: &Caller,
    id: &Value,
    read: &EventsRead,
    as_actor: Option<&str>,
) -> Value {
    if let Err(e) = events::authorize(caller, as_actor) {
        return tool_result(id, Err(e));
    }
    match call::events(env, &events::command(read)).await {
        Ok(LogReply::Events(list)) => tool_result_structured(
            id,
            events::render(read.after, &list),
            events::structured(read.after, &list),
        ),
        Ok(LogReply::Error(e)) | Err(e) => tool_result(id, Err(e)),
        Ok(other) => tool_result(id, Err(format!("unexpected reply {other:?}"))),
    }
}

async fn run_text(
    env: &Env,
    caller: &Caller,
    call: Invocation,
) -> std::result::Result<String, String> {
    let me = caller.resolve(call.as_actor.as_deref())?;
    let actor = me.actor.clone();
    let command = call.command;
    match command {
        Command::Whoami | Command::MailInbox { .. } | Command::EventsRead(_) => {
            Err("handled above".into())
        }
        Command::MailSend {
            to,
            subject,
            body,
            re,
        } => {
            let wake_map = settings::wake(env).unwrap_or_else(|e| {
                worker::console_error!("wake config: {e}");
                Default::default()
            });
            let envelope = Envelope {
                from: actor,
                to,
                subject,
                re,
                body,
            };
            let cmd = BusCmd::Send {
                envelope,
                directory: settings::mail_directory(env),
                wakeable: wake_map.actors(),
            };
            match bus(env, cmd).await? {
                BusReply::Sent { copies, wake: due } => {
                    wake::fire_all(&wake_map, &due).await;
                    Ok(tools::render_sent(&copies))
                }
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
            let compiled = settings::config(env).await.ok();
            let switches = settings::switches(env, compiled.as_ref())?;
            let limits = Limits {
                lane: switches.effective("Lane", &lane),
                device: switches.effective("Device", &me.claim),
            };
            let cmd = LeaseCmd::Acquire {
                lane,
                item: item.clone(),
                holder: me.claim.clone(),
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
                holder: me.claim.clone(),
            };
            match call::leases(env, &cmd).await? {
                LeaseReply::Lease(_) => Ok(format!("released {}#{}", item.repo, item.number)),
                LeaseReply::Error(e) => Err(e.detail()),
                other => Err(format!("unexpected reply {other:?}")),
            }
        }
    }
}
