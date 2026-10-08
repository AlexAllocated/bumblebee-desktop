//! Restart recovery resumes saved work only through the session's actor-owned queue.
use super::*;
use rusqlite::{TransactionBehavior, params};

impl Store {
	pub(crate) fn recovery_candidates(&self) -> Result<Vec<durable::TurnRecord>> {
		let ids = {
			let db = self.db()?;
			let mut query=db.prepare("SELECT id FROM agent_turns WHERE state='interrupted' AND json_extract(checkpoint,'$.recovery_eligible')=1 AND updated_at>=? ORDER BY updated_at LIMIT 64")?;
			query
				.query_map([crate::now_ms() - 86_400_000], |row| {
					row.get::<_, String>(0)
				})?
				.collect::<rusqlite::Result<Vec<_>>>()?
		};
		ids.iter().map(|id| self.turn(id)).collect()
	}
	pub(crate) fn stage_recovery(&self, id: &str, actor: &str) -> Result<bool> {
		Ok(self.db()?.execute("UPDATE agent_turns SET state='queued_recovery' WHERE id=? AND actor=? AND state='interrupted' AND json_extract(checkpoint,'$.recovery_eligible')=1 AND updated_at>=? AND NOT EXISTS(SELECT 1 FROM pending_inputs WHERE turn_id=? AND state='pending')",params![id,actor,crate::now_ms()-86_400_000,id])?==1)
	}
	fn claim_recovery(&self, id: &str) -> Result<bool> {
		Ok(self.db()?.execute(
			"UPDATE agent_turns SET state='running',updated_at=? WHERE id=? AND state='queued_recovery'",
			params![crate::now_ms(), id],
		)? == 1)
	}
	fn unstage_recovery(&self, id: &str) -> Result<()> {
		self.db()?.execute(
			"UPDATE agent_turns SET state='interrupted' WHERE id=? AND state='queued_recovery'",
			[id],
		)?;
		Ok(())
	}
	fn call_count(&self, id: &str) -> Result<usize> {
		Ok(self.db()?.query_row(
			"SELECT COUNT(*) FROM tool_calls WHERE turn_id=?",
			[id],
			|row| row.get(0),
		)?)
	}
	/// Explicit cancellation is terminal. Unknown delivery is retained for review,
	/// but never becomes resumable. The effect ledger remains intact.
	pub(crate) fn cancel_active_agent_work(&self, actor: Option<&str>) -> Result<()> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute("UPDATE tool_calls SET state='unknown' WHERE state='started' AND turn_id IN (SELECT id FROM agent_turns WHERE state IN ('running','queued_recovery') AND (?1 IS NULL OR actor=?1))",[actor])?;
		tx.execute("UPDATE agent_turns SET state='cancelled',updated_at=?1 WHERE (state IN ('running','queued_recovery') OR (state='interrupted' AND json_extract(checkpoint,'$.recovery_eligible')=1)) AND (?2 IS NULL OR actor=?2)",params![crate::now_ms(),actor])?;
		tx.execute("UPDATE agent_turns SET state='unknown',updated_at=?1 WHERE state='delivering' AND (?2 IS NULL OR actor=?2)",params![crate::now_ms(),actor])?;
		tx.commit()?;
		Ok(())
	}
}

pub(crate) fn recoverable_source(record: &durable::TurnRecord) -> Result<ChatMessage> {
	let cp: Checkpoint =
		serde_json::from_value(record.checkpoint.clone()).context("Saved request is not readable")?;
	ensure!(
		cp.id == record.id && durable::actor(&cp.source) == record.actor,
		"Saved request identity does not match its ledger"
	);
	ensure!(
		cp.recovery_eligible,
		"This older interrupted request has no safe recovery marker; inspect or cancel it"
	);
	ensure!(
		cp.pending.is_none(),
		"Saved questions must use their original answer path"
	);
	ensure!(
		cp.rounds <= 24 && cp.executed <= 96 && cp.cursor <= cp.calls.len(),
		"Saved request budgets are invalid"
	);
	Ok(cp.source)
}

pub(crate) async fn recover_interrupted(
	engine: Arc<Engine>,
	id: &str,
	cancel: CancellationToken,
) -> Result<()> {
	recover_with_host(&RuntimeHost { engine }, id, cancel).await
}

async fn recover_with_host<H: Host>(host: &H, id: &str, cancel: CancellationToken) -> Result<()> {
	let record = host.store().turn(id)?;
	ensure!(
		record.state == "queued_recovery",
		"This saved request is no longer queued for recovery"
	);
	if cancel.is_cancelled() {
		host.store().cancel_agent_turn(id)?;
		return Ok(());
	}
	let setup=async{
        ensure!(host.store().settings()?.ai_enabled,"Enable the agent before continuing interrupted requests");
        let _=recoverable_source(&record)?;
        let mut cp:Checkpoint=serde_json::from_value(record.checkpoint.clone())?;
        // Provider event badges are snapshots, not durable authorization. Owners
        // are revalidated live below; role-restricted viewers need a fresh message.
        cp.source.access=Default::default();cp.source.is_owner=false;
        tokio::select!{biased;_=cancel.cancelled()=>bail!("Request cancelled before recovery"),result=host.validate_checkpoint(&cp,&cancel)=>result?};
        let owner=tokio::select!{biased;_=cancel.cancelled()=>bail!("Request cancelled"),owner=host.owner(&cp.source)=>owner?};
        require_context_owner(&cp,owner)?;
        cp.executed=cp.executed.max(host.store().call_count(id)?);
        // Earlier checkpoints saved a validated terminal output just before the
        // delivering fence. Restore it instead of asking the model to redo work.
        if cp.pending_final.is_none() && cp.calls.is_empty() && cp.delivery.is_some(){
            if let Some(item)=cp.items.last().filter(|item|item["type"]=="message" && item["role"]=="assistant") {
                let (_,_,reply)=model::parse_response(json!({"status":"completed","output":[item]}))?;
                cp.pending_final=reply;
            }
        }
        Ok::<_,anyhow::Error>(cp)
    }.await;
	let mut cp = match setup {
		Ok(cp) => cp,
		Err(error) => {
			if cancel.is_cancelled() {
				host.store().cancel_agent_turn(id)?;
			} else {
				host.store().unstage_recovery(id)?;
			}
			return Err(error);
		}
	};
	if cancel.is_cancelled() {
		host.store().cancel_agent_turn(id)?;
		return Ok(());
	}
	ensure!(
		host.store().claim_recovery(id)?,
		"This interrupted request was already claimed"
	);
	run_guarded(host, &mut cp, cancel).await
}

pub(super) fn apply_saved_call<H: Host>(
	host: &H,
	cp: &mut Checkpoint,
	call: &ToolCall,
) -> Result<bool> {
	let Some((state, saved)) =
		host
			.store()
			.saved_call(&cp.id, &call.id, &call.name, &call.arguments)?
	else {
		return Ok(false);
	};
	ensure!(
		matches!(state.as_str(), "completed" | "unknown"),
		"Tool call is still running elsewhere"
	);
	let output = if state == "completed" {
		serde_json::from_str(&saved.context("Completed action is missing its observed result")?)?
	} else {
		saved.and_then(|value|serde_json::from_str::<Value>(&value).ok()).unwrap_or_else(||json!({"status":"unknown","error":"Bumblebee stopped while this operation was in progress. Its outcome is unknown. Inspect the destination; do not repeat this operation."}))
	};
	cp.executed = cp.executed.max(host.store().call_count(&cp.id)?);
	tools::apply_result(cp, &output)?;
	cp.answer = None;
	cp.approved_call = None;
	cp.items.push(json!({"type":"function_call_output","call_id":call.id,"output":json!({"untrustedToolResult":output}).to_string()}));
	cp.cursor += 1;
	checkpoint(host, cp, "running")?;
	Ok(true)
}

#[cfg(test)]
mod tests;
