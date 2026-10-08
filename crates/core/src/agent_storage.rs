//! Durable requester-scoped memories, questions, conversation and reminder claims.
use crate::{model::ChatMessage, now_ms, storage::Store};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn migrate(db: &Connection) -> Result<()> {
	db.execute_batch(
		"ALTER TABLE memories ADD COLUMN actor TEXT NOT NULL DEFAULT 'preview:owner';
        ALTER TABLE reminders ADD COLUMN actor TEXT NOT NULL DEFAULT 'preview:owner';
        ALTER TABLE reminders ADD COLUMN destination TEXT NOT NULL DEFAULT '{}';
        ALTER TABLE reminders ADD COLUMN state TEXT NOT NULL DEFAULT 'pending';
        ALTER TABLE reminders ADD COLUMN receipt TEXT;
        UPDATE reminders SET state='delivered' WHERE delivered=1;
        CREATE INDEX reminder_due ON reminders(state,due_at);
        CREATE TABLE pending_inputs (
            id TEXT PRIMARY KEY, turn_id TEXT NOT NULL UNIQUE REFERENCES agent_turns(id),
            actor TEXT NOT NULL, channel TEXT NOT NULL, kind TEXT NOT NULL,
            prompt TEXT NOT NULL, choices TEXT NOT NULL, owner_required INTEGER NOT NULL,
            expires_at INTEGER NOT NULL, state TEXT NOT NULL, answer TEXT);
        CREATE INDEX pending_actor ON pending_inputs(actor,channel,state,expires_at);
        CREATE TABLE conversation_history (
            id INTEGER PRIMARY KEY, scope TEXT NOT NULL, role TEXT NOT NULL,
            content TEXT NOT NULL, created_at INTEGER NOT NULL);
        CREATE INDEX history_scope ON conversation_history(scope,id);",
	)?;
	Ok(())
}
pub fn platform(p: &str) -> &str {
	if p == "discord_voice" { "discord" } else { p }
}
pub fn actor(m: &ChatMessage) -> String {
	format!("{}:{}", platform(&m.platform), m.user_id)
}
pub fn channel(m: &ChatMessage) -> String {
	format!("{}:{}", platform(&m.platform), m.channel_id)
}
pub fn conversation_scope(m: &ChatMessage) -> String {
	format!("{}:{}", actor(m), channel(m))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
	pub id: String,
	pub content: String,
	pub actor: String,
	pub created_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
	pub id: String,
	pub content: String,
	pub actor: String,
	pub due_at: i64,
	pub state: String,
	pub destination: ChatMessage,
	pub receipt: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingInput {
	pub id: String,
	pub turn_id: String,
	pub actor: String,
	pub channel: String,
	pub kind: String,
	pub prompt: String,
	pub choices: Vec<String>,
	pub owner_required: bool,
	pub expires_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
	pub id: String,
	pub actor: String,
	pub state: String,
	pub checkpoint: Value,
	pub updated_at: i64,
}

impl Store {
	pub fn turn(&self, id: &str) -> Result<TurnRecord> {
		let db = self.db()?;
		let (id, actor, state, checkpoint, updated_at): (String, String, String, String, i64) = db
			.query_row(
				"SELECT id,actor,state,checkpoint,updated_at FROM agent_turns WHERE id=?",
				[id],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
			)?;
		Ok(TurnRecord {
			id,
			actor,
			state,
			checkpoint: serde_json::from_str(&checkpoint)?,
			updated_at,
		})
	}
	pub fn interrupted_turns(&self) -> Result<Vec<TurnRecord>> {
		let ids = {
			let db = self.db()?;
			let mut stmt=db.prepare("SELECT id FROM agent_turns WHERE state IN ('interrupted','unknown') ORDER BY updated_at DESC LIMIT 64")?;
			stmt
				.query_map([], |r| r.get::<_, String>(0))?
				.collect::<rusqlite::Result<Vec<_>>>()?
		};
		ids.iter().map(|id| self.turn(id)).collect()
	}
	pub fn suspend_turn(&self, input: &PendingInput, checkpoint: &Value) -> Result<()> {
		ensure!(
			matches!(input.kind.as_str(), "confirmation" | "question"),
			"Invalid pending input kind"
		);
		ensure!(
			!input.prompt.trim().is_empty() && input.prompt.len() <= 4000,
			"Invalid pending prompt"
		);
		ensure!(
			input.choices.len() <= 20
				&& input
					.choices
					.iter()
					.all(|s| !s.is_empty() && s.len() <= 200),
			"Invalid pending choices"
		);
		let now = now_ms();
		ensure!(
			input.expires_at > now && input.expires_at <= now + 86_400_000,
			"Invalid pending expiration"
		);
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute(
			"UPDATE pending_inputs SET state='expired' WHERE state='pending' AND expires_at<=?",
			[now],
		)?;
		let count: i64 = tx.query_row(
			"SELECT COUNT(*) FROM pending_inputs WHERE state='pending'",
			[],
			|r| r.get(0),
		)?;
		ensure!(
			count < 64,
			"Bumblebee already has 64 pending questions; resolve or cancel one first"
		);
		let turn_actor: String = tx.query_row(
			"SELECT actor FROM agent_turns WHERE id=?",
			[&input.turn_id],
			|r| r.get(0),
		)?;
		ensure!(
			turn_actor == input.actor,
			"Pending input actor does not own this turn"
		);
		tx.execute("INSERT INTO pending_inputs(id,turn_id,actor,channel,kind,prompt,choices,owner_required,expires_at,state)
            VALUES (?,?,?,?,?,?,?,?,?,'pending') ON CONFLICT(turn_id) DO UPDATE SET
            id=excluded.id,actor=excluded.actor,channel=excluded.channel,kind=excluded.kind,prompt=excluded.prompt,
            choices=excluded.choices,owner_required=excluded.owner_required,expires_at=excluded.expires_at,state='pending',answer=NULL",
            params![input.id,input.turn_id,input.actor,input.channel,input.kind,input.prompt,serde_json::to_string(&input.choices)?,input.owner_required,input.expires_at])?;
		ensure!(
            tx.execute(
                "UPDATE agent_turns SET state='waiting',checkpoint=?,updated_at=? WHERE id=? AND state='running'",
                params![checkpoint.to_string(), now, input.turn_id]
            )? == 1,
            "Turn no longer exists"
        );
		tx.commit()?;
		Ok(())
	}
	pub fn pending_inputs(&self) -> Result<Vec<PendingInput>> {
		let db = self.db()?;
		let mut stmt=db.prepare("SELECT id,turn_id,actor,channel,kind,prompt,choices,owner_required,expires_at FROM pending_inputs WHERE state='pending' AND expires_at>? ORDER BY expires_at LIMIT 64")?;
		let rows = stmt
			.query_map([now_ms()], |r| {
				Ok((
					r.get::<_, String>(0)?,
					r.get::<_, String>(1)?,
					r.get::<_, String>(2)?,
					r.get::<_, String>(3)?,
					r.get::<_, String>(4)?,
					r.get::<_, String>(5)?,
					r.get::<_, String>(6)?,
					r.get::<_, bool>(7)?,
					r.get::<_, i64>(8)?,
				))
			})?
			.collect::<rusqlite::Result<Vec<_>>>()?;
		rows
			.into_iter()
			.map(
				|(id, turn_id, actor, channel, kind, prompt, choices, owner_required, expires_at)| {
					Ok(PendingInput {
						id,
						turn_id,
						actor,
						channel,
						kind,
						prompt,
						choices: serde_json::from_str(&choices)?,
						owner_required,
						expires_at,
					})
				},
			)
			.collect()
	}
	/// Consume once while saving the exact resumed checkpoint before any effect.
	/// The caller must recheck live provider ownership before asserting is_owner.
	pub fn consume_pending(
		&self,
		id: &str,
		actor: &str,
		channel: &str,
		is_owner: bool,
		answer: &str,
		checkpoint: &Value,
	) -> Result<String> {
		ensure!(answer.len() <= 8000, "Answer is too long");
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let row: Option<(String, String, String, bool, i64, String)> = tx
			.query_row(
				"SELECT turn_id,actor,channel,owner_required,expires_at,state FROM pending_inputs WHERE id=?",
				[id],
				|r| {
					Ok((
						r.get(0)?,
						r.get(1)?,
						r.get(2)?,
						r.get(3)?,
						r.get(4)?,
						r.get(5)?,
					))
				},
			)
			.optional()?;
		let (turn, bound_actor, bound_channel, requires_owner, expires, state) =
			row.context("This question no longer exists")?;
		ensure!(
			bound_actor == actor && bound_channel == channel,
			"This answer belongs to another requester or channel"
		);
		ensure!(
			state == "pending" && expires > now_ms(),
			"This question expired or was already answered"
		);
		ensure!(
			!requires_owner || is_owner,
			"Owner permission changed; this action remains unapproved"
		);
		tx.execute(
			"UPDATE pending_inputs SET state='consumed',answer=? WHERE id=?",
			params![answer, id],
		)?;
		tx.execute(
			"UPDATE agent_turns SET state='running',checkpoint=?,updated_at=? WHERE id=?",
			params![checkpoint.to_string(), now_ms(), turn],
		)?;
		tx.commit()?;
		Ok(turn)
	}
	pub fn cancel_agent_turn(&self, turn: &str) -> Result<()> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute(
			"UPDATE pending_inputs SET state='cancelled' WHERE turn_id=? AND state='pending'",
			[turn],
		)?;
		tx.execute(
			"UPDATE tool_calls SET state='unknown' WHERE turn_id=? AND state='started'",
			[turn],
		)?;
		tx.execute(
			"UPDATE agent_turns SET state='cancelled',updated_at=? WHERE id=? AND state!='completed'",
			params![now_ms(), turn],
		)?;
		tx.commit()?;
		Ok(())
	}
	pub fn memories(&self, actor: &str, owner: bool) -> Result<Vec<Memory>> {
		let db = self.db()?;
		let mut stmt=db.prepare("SELECT id,content,actor,created_at FROM memories WHERE actor=? OR ?=1 ORDER BY created_at DESC LIMIT 200")?;
		Ok(stmt
			.query_map(params![actor, owner], |r| {
				Ok(Memory {
					id: r.get(0)?,
					content: r.get(1)?,
					actor: r.get(2)?,
					created_at: r.get(3)?,
				})
			})?
			.collect::<rusqlite::Result<Vec<_>>>()?)
	}
	pub fn remember(&self, actor: &str, content: &str) -> Result<Memory> {
		ensure!(
			!content.trim().is_empty() && content.len() <= 4000,
			"Memory must contain 1-4000 bytes"
		);
		let m = Memory {
			id: uuid::Uuid::new_v4().to_string(),
			content: content.into(),
			actor: actor.into(),
			created_at: now_ms(),
		};
		self.db()?.execute(
			"INSERT INTO memories(id,content,created_at,actor) VALUES (?,?,?,?)",
			params![m.id, m.content, m.created_at, m.actor],
		)?;
		Ok(m)
	}
	pub fn update_memory(&self, id: &str, actor: &str, owner: bool, content: &str) -> Result<()> {
		ensure!(
			!content.trim().is_empty() && content.len() <= 4000,
			"Memory must contain 1-4000 bytes"
		);
		ensure!(
			self.db()?.execute(
				"UPDATE memories SET content=? WHERE id=? AND (actor=? OR ?=1)",
				params![content, id, actor, owner]
			)? == 1,
			"Memory does not exist or belongs to another requester"
		);
		Ok(())
	}
	pub fn delete_memory(&self, id: &str, actor: &str, owner: bool) -> Result<()> {
		ensure!(
			self.db()?.execute(
				"DELETE FROM memories WHERE id=? AND (actor=? OR ?=1)",
				params![id, actor, owner]
			)? == 1,
			"Memory does not exist or belongs to another requester"
		);
		Ok(())
	}
	pub fn create_reminder(
		&self,
		message: &ChatMessage,
		content: &str,
		due_at: i64,
	) -> Result<Reminder> {
		ensure!(
			platform(&message.platform) == "discord",
			"Reminders require Discord so Bumblebee can DM their requester"
		);
		ensure!(
			!content.trim().is_empty() && content.len() <= 2000,
			"Reminder must contain 1-2000 bytes"
		);
		ensure!(
			due_at > now_ms() && due_at <= now_ms() + 366 * 86_400_000,
			"Reminder time must be within the next year"
		);
		let r = Reminder {
			id: uuid::Uuid::new_v4().to_string(),
			content: content.into(),
			actor: actor(message),
			due_at,
			state: "pending".into(),
			destination: message.clone(),
			receipt: None,
		};
		self.db()?.execute("INSERT INTO reminders(id,content,due_at,delivered,actor,destination,state) VALUES (?,?,?,0,?,?,'pending')",params![r.id,r.content,r.due_at,r.actor,serde_json::to_string(message)?])?;
		Ok(r)
	}
	pub fn reminders(&self, actor: &str, owner: bool) -> Result<Vec<Reminder>> {
		let db = self.db()?;
		let mut stmt=db.prepare("SELECT id,content,actor,due_at,state,destination,receipt FROM reminders WHERE (actor=? OR ?=1) AND state!='cancelled' ORDER BY due_at LIMIT 200")?;
		let rows = stmt
			.query_map(params![actor, owner], read_reminder)?
			.collect::<rusqlite::Result<Vec<_>>>()?;
		rows.into_iter().map(reminder_from_row).collect()
	}
	pub fn update_reminder(
		&self,
		id: &str,
		actor: &str,
		owner: bool,
		content: &str,
		due_at: i64,
	) -> Result<()> {
		ensure!(
			!content.trim().is_empty() && content.len() <= 2000,
			"Reminder must contain 1-2000 bytes"
		);
		ensure!(
			due_at > now_ms() && due_at <= now_ms() + 366 * 86_400_000,
			"Reminder time must be within the next year"
		);
		ensure!(
			self.db()?.execute(
				"UPDATE reminders SET content=?,due_at=? WHERE id=? AND (actor=? OR ?=1) AND state='pending'",
				params![content, due_at, id, actor, owner]
			)? == 1,
			"Reminder does not exist, belongs to another requester, or was already dispatched"
		);
		Ok(())
	}
	pub fn cancel_reminder(&self, id: &str, actor: &str, owner: bool) -> Result<()> {
		ensure!(
			self.db()?.execute(
				"UPDATE reminders SET state='cancelled' WHERE id=? AND (actor=? OR ?=1) AND state='pending'",
				params![id, actor, owner]
			)? == 1,
			"Reminder does not exist, belongs to another requester, or was already dispatched"
		);
		Ok(())
	}
	pub fn claim_due_reminder(&self, now: i64) -> Result<Option<Reminder>> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let row=tx.query_row("SELECT id,content,actor,due_at,state,destination,receipt FROM reminders WHERE state='pending' AND due_at<=? ORDER BY due_at LIMIT 1",[now],read_reminder).optional()?;
		let Some(row) = row else { return Ok(None) };
		let mut r = reminder_from_row(row)?;
		tx.execute(
			"UPDATE reminders SET state='dispatching' WHERE id=?",
			[&r.id],
		)?;
		tx.commit()?;
		r.state = "dispatching".into();
		Ok(Some(r))
	}
	pub fn finish_reminder(&self, id: &str, receipt: Option<&str>) -> Result<()> {
		let state = if receipt.is_some() {
			"delivered"
		} else {
			"unknown"
		};
		ensure!(
			self.db()?.execute(
				"UPDATE reminders SET state=?,receipt=?,delivered=? WHERE id=? AND state='dispatching'",
				params![state, receipt, receipt.is_some(), id]
			)? == 1,
			"Reminder is not dispatching"
		);
		Ok(())
	}
	pub fn recover_agent_state(&self) -> Result<()> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute(
			"UPDATE reminders SET state='unknown' WHERE state='dispatching'",
			[],
		)?;
		tx.execute("UPDATE agent_turns SET state='expired',updated_at=? WHERE id IN (SELECT turn_id FROM pending_inputs WHERE state='pending' AND expires_at<=?)",params![now_ms(),now_ms()])?;
		tx.execute(
			"UPDATE pending_inputs SET state='expired' WHERE state='pending' AND expires_at<=?",
			[now_ms()],
		)?;
		tx.commit()?;
		Ok(())
	}
	pub fn append_history(&self, scope: &str, role: &str, content: &str) -> Result<()> {
		ensure!(matches!(role, "user" | "assistant"), "Invalid history role");
		let content = content.chars().take(12000).collect::<String>();
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute(
			"INSERT INTO conversation_history(scope,role,content,created_at) VALUES (?,?,?,?)",
			params![scope, role, content, now_ms()],
		)?;
		tx.execute("DELETE FROM conversation_history WHERE scope=?1 AND id NOT IN (SELECT id FROM conversation_history WHERE scope=?1 ORDER BY id DESC LIMIT 40)",[scope])?;
		tx.execute(
			"DELETE FROM conversation_history WHERE created_at<?",
			[now_ms() - 30 * 86_400_000],
		)?;
		tx.commit()?;
		Ok(())
	}
	pub fn history(&self, scope: &str) -> Result<Vec<Value>> {
		let db = self.db()?;
		let mut stmt = db.prepare(
			"SELECT role,content FROM conversation_history WHERE scope=? ORDER BY id DESC LIMIT 40",
		)?;
		let mut rows = stmt
			.query_map([scope], |r| {
				Ok(serde_json::json!({"role":r.get::<_,String>(0)?,"content":r.get::<_,String>(1)?}))
			})?
			.collect::<rusqlite::Result<Vec<_>>>()?;
		rows.reverse();
		Ok(rows)
	}
	pub fn clear_history(&self, scope: &str) -> Result<usize> {
		Ok(self
			.db()?
			.execute("DELETE FROM conversation_history WHERE scope=?", [scope])?)
	}
}
type ReminderRow = (String, String, String, i64, String, String, Option<String>);
fn read_reminder(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReminderRow> {
	Ok((
		r.get(0)?,
		r.get(1)?,
		r.get(2)?,
		r.get(3)?,
		r.get(4)?,
		r.get(5)?,
		r.get(6)?,
	))
}
fn reminder_from_row(
	(id, content, actor, due_at, state, destination, receipt): ReminderRow,
) -> Result<Reminder> {
	Ok(Reminder {
		id,
		content,
		actor,
		due_at,
		state,
		destination: serde_json::from_str(&destination)?,
		receipt,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	fn source() -> ChatMessage {
		ChatMessage {
			platform: "discord".into(),
			user_id: "1".into(),
			display_name: "Owner".into(),
			message_id: "m".into(),
			channel_id: "room".into(),
			text: "hello".into(),
			is_owner: true,
			access: Default::default(),
		}
	}
	#[test]
	fn pending_input_is_bound_survives_restart_and_consumes_exactly_once() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		let m = source();
		let context = serde_json::json!({"remaining":["ban","announce"],"target":"saved-user"});
		{
			let s = Store::open(&path).unwrap();
			s.create_turn("t", &actor(&m), &context).unwrap();
			s.suspend_turn(
				&PendingInput {
					id: "code".into(),
					turn_id: "t".into(),
					actor: actor(&m),
					channel: channel(&m),
					kind: "confirmation".into(),
					prompt: "Ban saved-user?".into(),
					choices: vec!["yes".into(), "no".into()],
					owner_required: true,
					expires_at: now_ms() + 60000,
				},
				&context,
			)
			.unwrap();
		}
		let s = Store::open(&path).unwrap();
		s.recover_interrupted().unwrap();
		s.recover_agent_state().unwrap();
		assert_eq!(s.pending_inputs().unwrap().len(), 1);
		assert!(
			s.consume_pending("code", "discord:other", &channel(&m), true, "yes", &context)
				.is_err()
		);
		assert!(
			s.consume_pending("code", &actor(&m), "discord:other", true, "yes", &context)
				.is_err()
		);
		assert!(
			s.consume_pending("code", &actor(&m), &channel(&m), false, "yes", &context)
				.is_err()
		);
		assert_eq!(s.turn("t").unwrap().checkpoint, context);
		s.consume_pending("code", &actor(&m), &channel(&m), true, "yes", &context)
			.unwrap();
		assert!(
			s.consume_pending("code", &actor(&m), &channel(&m), true, "yes", &context)
				.is_err()
		);
		assert!(s.pending_inputs().unwrap().is_empty());
	}
	#[test]
	fn memories_and_history_do_not_cross_requester_or_channel_boundaries() {
		let s = Store::open(std::path::Path::new(":memory:")).unwrap();
		let memory = s.remember("discord:a", "a private preference").unwrap();
		assert!(s.memories("discord:b", false).unwrap().is_empty());
		assert!(
			s.update_memory(&memory.id, "discord:b", false, "overwrite")
				.is_err()
		);
		assert!(s.delete_memory(&memory.id, "discord:b", false).is_err());
		s.update_memory(&memory.id, "discord:owner", true, "corrected")
			.unwrap();
		assert_eq!(
			s.memories("discord:a", false).unwrap()[0].content,
			"corrected"
		);
		s.append_history("private-a", "user", "private").unwrap();
		assert!(s.history("public-a").unwrap().is_empty());
	}
	#[test]
	fn interrupted_reminder_delivery_is_unknown_and_not_claimed_again() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		let due = now_ms() + 10000;
		let id = {
			let s = Store::open(&path).unwrap();
			let r = s.create_reminder(&source(), "Take a break", due).unwrap();
			assert_eq!(s.claim_due_reminder(due).unwrap().unwrap().id, r.id);
			r.id
		};
		let s = Store::open(&path).unwrap();
		s.recover_agent_state().unwrap();
		assert!(s.claim_due_reminder(due + 1000).unwrap().is_none());
		let r = s.reminders("discord:1", false).unwrap();
		assert_eq!(r[0].id, id);
		assert_eq!(r[0].state, "unknown");
	}
}

#[cfg(test)]
mod reminder_edit_tests {
	use super::*;
	#[test]
	fn edits_preserve_requester_scope_and_cannot_race_a_claimed_delivery() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::open(&dir.path().join("app.db")).unwrap();
		let source = ChatMessage {
			platform: "discord".into(),
			user_id: "123".into(),
			display_name: "Viewer".into(),
			message_id: "m".into(),
			channel_id: "room".into(),
			text: "remind me".into(),
			is_owner: false,
			access: Default::default(),
		};
		let due = now_ms() + 60000;
		let reminder = store.create_reminder(&source, "Old text", due).unwrap();
		assert!(
			store
				.update_reminder(&reminder.id, "discord:456", false, "Not mine", due)
				.is_err()
		);
		store
			.update_reminder(&reminder.id, &actor(&source), false, "New text", due + 1000)
			.unwrap();
		assert!(store.claim_due_reminder(due).unwrap().is_none());
		let claimed = store.claim_due_reminder(due + 1000).unwrap().unwrap();
		assert_eq!(claimed.content, "New text");
		assert!(
			store
				.update_reminder(&reminder.id, &actor(&source), true, "Too late", due + 2000)
				.is_err()
		);
		store.finish_reminder(&reminder.id, None).unwrap();
		assert!(
			store
				.update_reminder(
					&reminder.id,
					&actor(&source),
					true,
					"Retry unknown",
					due + 3000
				)
				.is_err()
		);
		assert!(store.claim_due_reminder(due + 3000).unwrap().is_none());
	}
}
