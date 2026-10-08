use crate::{catalog, images, model::*, now_ms};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
	path::Path,
	sync::{Mutex, MutexGuard},
};

const SCHEMA_VERSION: i64 = 4;

/// Only explicitly classified pure reads may bypass a prior uncertain effect.
/// Local writes are mutations even when they never call an external provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallEffect {
	ReadOnly,
	MayMutate,
}

pub struct Store {
	pub(crate) connection: Mutex<Connection>,
}

impl Store {
	pub fn open(path: &Path) -> Result<Self> {
		let mut conn = Connection::open(path)?;
		conn.busy_timeout(std::time::Duration::from_secs(5))?;
		let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
		ensure!(
			version <= SCHEMA_VERSION,
			"This database was created by a newer Bumblebee version; update the application to open it"
		);
		conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
		// Recheck under the write lock so simultaneous opens cannot repeat a migration.
		let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
		ensure!(
			version <= SCHEMA_VERSION,
			"Database version is newer than this application"
		);
		if version < 1 {
			tx.execute_batch("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE chatters (
                    platform TEXT NOT NULL, user_id TEXT NOT NULL, display_name TEXT NOT NULL,
                    puppet_id TEXT NOT NULL, voice_id TEXT NOT NULL, image_hash TEXT,
                    customization_blocked INTEGER NOT NULL DEFAULT 0,
                    PRIMARY KEY(platform,user_id));
                CREATE TABLE image_submissions (
                    id TEXT PRIMARY KEY, platform TEXT NOT NULL, user_id TEXT NOT NULL,
                    image_hash TEXT NOT NULL, submitted_at INTEGER NOT NULL,
                    UNIQUE(platform,user_id), FOREIGN KEY(platform,user_id) REFERENCES chatters(platform,user_id));
                CREATE TABLE memories (id TEXT PRIMARY KEY, content TEXT NOT NULL, created_at INTEGER NOT NULL);
                CREATE TABLE reminders (id TEXT PRIMARY KEY, content TEXT NOT NULL, due_at INTEGER NOT NULL, delivered INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE agent_turns (id TEXT PRIMARY KEY, actor TEXT NOT NULL, state TEXT NOT NULL, checkpoint TEXT NOT NULL, updated_at INTEGER NOT NULL);
                CREATE TABLE tool_calls (turn_id TEXT NOT NULL, call_id TEXT NOT NULL, name TEXT NOT NULL, args TEXT NOT NULL, state TEXT NOT NULL, result TEXT, PRIMARY KEY(turn_id,call_id), FOREIGN KEY(turn_id) REFERENCES agent_turns(id));
                CREATE TABLE processed_messages (platform TEXT NOT NULL, message_id TEXT NOT NULL, processed_at INTEGER NOT NULL, PRIMARY KEY(platform,message_id));
                CREATE TABLE command_cooldowns (platform TEXT NOT NULL, user_id TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(platform,user_id));")?;
		}
		if version < 2 {
			tx.execute_batch("CREATE TABLE image_requests (
                platform TEXT NOT NULL, user_id TEXT NOT NULL, id TEXT NOT NULL UNIQUE,
                PRIMARY KEY(platform,user_id), FOREIGN KEY(platform,user_id) REFERENCES chatters(platform,user_id));
                CREATE INDEX tool_calls_action_state ON tool_calls(name,args,state);")?;
			let calls = {
				let mut stmt = tx.prepare("SELECT turn_id,call_id,args FROM tool_calls")?;
				stmt
					.query_map([], |r| {
						Ok((
							r.get::<_, String>(0)?,
							r.get::<_, String>(1)?,
							r.get::<_, String>(2)?,
						))
					})?
					.collect::<rusqlite::Result<Vec<_>>>()?
			};
			for (turn, call, args) in calls {
				if let Ok(args) = canonical_args(&args) {
					tx.execute(
						"UPDATE tool_calls SET args=? WHERE turn_id=? AND call_id=?",
						params![args, turn, call],
					)?;
				}
			}
		}
		if version < 3 {
			crate::agent_storage::migrate(&tx)?;
		}
		if version < 4 {
			tx.execute_batch("ALTER TABLE chatters ADD COLUMN overrides TEXT NOT NULL DEFAULT '{}';")?;
			for key in ["installation", "overlay_settings"] {
				let old: Option<String> = tx
					.query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
						r.get(0)
					})
					.optional()?;
				if let Some(old) = old {
					let next = if key == "installation" {
						serde_json::to_value(serde_json::from_str::<Settings>(&old)?)?
					} else {
						serde_json::to_value(crate::overlay::migrate_legacy(serde_json::from_str(
							&old,
						)?)?)?
					};
					tx.execute(
						"UPDATE settings SET value=? WHERE key=?",
						params![next.to_string(), key],
					)?;
				}
			}
			let undo: Option<String> = tx
				.query_row(
					"SELECT value FROM settings WHERE key='settings_undo'",
					[],
					|r| r.get(0),
				)
				.optional()?;
			if let Some(undo) = undo {
				let mut undo: serde_json::Value = serde_json::from_str(&undo)?;
				for side in ["before", "after"] {
					if undo["key"] == "overlay_settings" {
						undo[side] =
							serde_json::to_value(crate::overlay::migrate_legacy(undo[side].clone())?)?;
					} else if undo["key"] == "installation" {
						undo[side] =
							serde_json::to_value(serde_json::from_value::<Settings>(undo[side].clone())?)?;
					}
				}
				tx.execute(
					"UPDATE settings SET value=? WHERE key='settings_undo'",
					[undo.to_string()],
				)?;
			}
		}
		tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
		tx.commit()?;
		Ok(Self {
			connection: Mutex::new(conn),
		})
	}
	pub(crate) fn db(&self) -> Result<MutexGuard<'_, Connection>> {
		self
			.connection
			.lock()
			.map_err(|_| anyhow::anyhow!("Database lock poisoned"))
	}
	pub fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
		let value: Option<String> = self
			.db()?
			.query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
				r.get(0)
			})
			.optional()?;
		value
			.map(|v| serde_json::from_str(&v).map_err(Into::into))
			.transpose()
	}
	pub fn set<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
		self.db()?.execute("INSERT INTO settings(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, serde_json::to_string(value)?])?;
		Ok(())
	}
	pub fn settings(&self) -> Result<Settings> {
		Ok(self.get("installation")?.unwrap_or_default())
	}
	pub fn overlay_token(&self) -> Result<String> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let old: Option<String> = tx
			.query_row(
				"SELECT value FROM settings WHERE key='overlay_token'",
				[],
				|r| r.get(0),
			)
			.optional()?;
		if let Some(old) = old {
			return Ok(serde_json::from_str(&old)?);
		}
		let token = format!(
			"{}{}",
			uuid::Uuid::new_v4().simple(),
			uuid::Uuid::new_v4().simple()
		);
		tx.execute(
			"INSERT INTO settings VALUES ('overlay_token',?)",
			[serde_json::to_string(&token)?],
		)?;
		tx.commit()?;
		Ok(token)
	}
	pub fn catalog_voices(&self) -> Result<Vec<Voice>> {
		let mut result = catalog::voices();
		result.extend(self.get::<Vec<Voice>>("azure_voices")?.unwrap_or_default());
		Ok(result)
	}
	pub fn ensure_chatter(
		&self,
		platform: &str,
		user_id: &str,
		display_name: &str,
	) -> Result<Chatter> {
		ensure!(
			["twitch", "youtube", "discord", "preview"].contains(&platform),
			"Unsupported platform"
		);
		ensure!(
			!user_id.is_empty() && user_id.len() <= 128,
			"Invalid platform user ID"
		);
		ensure!(display_name.len() <= 1024, "Display name is too long");
		let puppets = catalog::puppets();
		let voices = catalog::eligible_voices(&self.catalog_voices()?);
		ensure!(
			!puppets.is_empty() && !voices.is_empty(),
			"Catalog unavailable"
		);
		let db = self.db()?;
		db.execute("INSERT INTO chatters(platform,user_id,display_name,puppet_id,voice_id) VALUES (?,?,?,?,?) ON CONFLICT(platform,user_id) DO UPDATE SET display_name=excluded.display_name", params![platform, user_id, display_name, puppets[catalog::random_index(puppets.len())].id, voices[catalog::random_index(voices.len())].id])?;
		Ok(db.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?)
	}
	pub fn chatter(&self, platform: &str, user_id: &str) -> Result<Chatter> {
		Ok(self.db()?.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?)
	}
	pub fn chatters(&self, search: &str) -> Result<Vec<Chatter>> {
		let db = self.db()?;
		let mut stmt = db.prepare("SELECT * FROM chatters WHERE instr(lower(display_name),lower(?1))>0 OR instr(lower(user_id),lower(?1))>0 ORDER BY display_name,platform,user_id LIMIT 200")?;
		Ok(stmt
			.query_map([search], read_chatter)?
			.collect::<rusqlite::Result<Vec<_>>>()?)
	}
	pub fn set_chatter_overrides(
		&self,
		platform: &str,
		user_id: &str,
		overrides: &ChatterOverrides,
	) -> Result<Chatter> {
		overrides.validate()?;
		let db = self.db()?;
		ensure!(
			db.execute(
				"UPDATE chatters SET overrides=? WHERE platform=? AND user_id=?",
				params![serde_json::to_string(overrides)?, platform, user_id]
			)? == 1,
			"Chatter profile does not exist"
		);
		Ok(db.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?)
	}
	pub fn patch_settings(&self, patch: &serde_json::Value) -> Result<Settings> {
		fn rejects_null(value: &serde_json::Value) -> bool {
			value.is_null()
				|| value
					.as_object()
					.is_some_and(|map| map.values().any(rejects_null))
				|| value
					.as_array()
					.is_some_and(|list| list.iter().any(rejects_null))
		}
		ensure!(!rejects_null(patch), "Settings patches cannot contain null");
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let old: Option<String> = tx
			.query_row(
				"SELECT value FROM settings WHERE key='installation'",
				[],
				|r| r.get(0),
			)
			.optional()?;
		let before = old
			.map(|s| serde_json::from_str::<Settings>(&s))
			.transpose()?
			.unwrap_or_default();
		let next = before.patched(patch)?;
		tx.execute("INSERT INTO settings(key,value) VALUES ('installation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&next)?])?;
		tx.commit()?;
		Ok(next)
	}
	pub fn patch_overlay(&self, patch: &serde_json::Value) -> Result<OverlaySettings> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let old: Option<String> = tx
			.query_row(
				"SELECT value FROM settings WHERE key='overlay_settings'",
				[],
				|r| r.get(0),
			)
			.optional()?;
		let before = old
			.map(|s| serde_json::from_str::<OverlaySettings>(&s))
			.transpose()?
			.unwrap_or_default();
		let mut next = serde_json::to_value(&before)?;
		crate::settings::merge_patch(&mut next, patch)?;
		let next: OverlaySettings = serde_json::from_value(next)?;
		next.validate()?;
		tx.execute("INSERT INTO settings(key,value) VALUES ('overlay_settings',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[serde_json::to_string(&next)?])?;
		tx.commit()?;
		Ok(next)
	}
	pub fn select_puppet(&self, platform: &str, user_id: &str, puppet: &str) -> Result<Chatter> {
		self.change_puppet(platform, user_id, puppet, false)
	}
	/// Trusted streamer action; viewer customization blocks do not prevent moderation.
	pub fn set_chatter_puppet(
		&self,
		platform: &str,
		user_id: &str,
		puppet: &str,
	) -> Result<Chatter> {
		self.change_puppet(platform, user_id, puppet, true)
	}
	fn change_puppet(
		&self,
		platform: &str,
		user_id: &str,
		puppet: &str,
		streamer: bool,
	) -> Result<Chatter> {
		ensure!(
			catalog::puppets().iter().any(|p| p.id == puppet),
			"Unknown puppet"
		);
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		ensure!(tx.execute("UPDATE chatters SET puppet_id=?,image_hash=NULL WHERE platform=? AND user_id=? AND (customization_blocked=0 OR ?)", params![puppet, platform, user_id, streamer])? == 1, "Profile does not exist or customization is blocked");
		clear_image_requests(&tx, platform, user_id)?;
		let chatter = tx.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?;
		tx.commit()?;
		Ok(chatter)
	}
	pub fn select_voice(&self, platform: &str, user_id: &str, voice: &str) -> Result<Chatter> {
		self.change_voice(platform, user_id, voice, false)
	}
	/// Trusted streamer action; validates the same chatter-only catalog as viewer selection.
	pub fn set_chatter_voice(&self, platform: &str, user_id: &str, voice: &str) -> Result<Chatter> {
		self.change_voice(platform, user_id, voice, true)
	}
	fn change_voice(
		&self,
		platform: &str,
		user_id: &str,
		voice: &str,
		streamer: bool,
	) -> Result<Chatter> {
		let voices = catalog::eligible_voices(&self.catalog_voices()?);
		let selected = catalog::resolve_voice(&voices, voice)?;
		let db = self.db()?;
		ensure!(
			db.execute(
				"UPDATE chatters SET voice_id=? WHERE platform=? AND user_id=? AND (customization_blocked=0 OR ?)",
				params![selected.id, platform, user_id, streamer]
			)? == 1,
			"Profile does not exist or customization is blocked"
		);
		Ok(db.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?)
	}
	/// Streamer action: restore built-in choices and invalidate pending downloads.
	/// Blocking is independent and remains until explicitly removed.
	pub fn reset_chatter(&self, platform: &str, user_id: &str) -> Result<Chatter> {
		let puppets = catalog::puppets();
		let voices = catalog::eligible_voices(&self.catalog_voices()?);
		ensure!(
			!puppets.is_empty() && !voices.is_empty(),
			"Catalog unavailable"
		);
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		ensure!(
			tx.execute(
				"UPDATE chatters SET puppet_id=?,voice_id=?,image_hash=NULL WHERE platform=? AND user_id=?",
				params![
					puppets[catalog::random_index(puppets.len())].id,
					voices[catalog::random_index(voices.len())].id,
					platform,
					user_id
				]
			)? == 1,
			"Profile does not exist"
		);
		clear_image_requests(&tx, platform, user_id)?;
		let chatter = tx.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			read_chatter,
		)?;
		tx.commit()?;
		Ok(chatter)
	}
	pub fn block_customization(&self, platform: &str, user_id: &str, blocked: bool) -> Result<()> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		ensure!(
			tx.execute(
				"UPDATE chatters SET customization_blocked=? WHERE platform=? AND user_id=?",
				params![blocked, platform, user_id]
			)? == 1,
			"Profile does not exist"
		);
		if blocked {
			clear_image_requests(&tx, platform, user_id)?;
		}
		tx.commit()?;
		Ok(())
	}
	/// Issue before fetching. Newer commands supersede older HTTP responses.
	pub fn begin_image_submission(&self, platform: &str, user_id: &str) -> Result<String> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		ensure_image_customization(&tx, platform, user_id)?;
		let id = uuid::Uuid::new_v4().to_string();
		tx.execute("INSERT INTO image_requests(platform,user_id,id) VALUES (?,?,?) ON CONFLICT(platform,user_id) DO UPDATE SET id=excluded.id", params![platform, user_id, id])?;
		tx.execute(
			"DELETE FROM image_submissions WHERE platform=? AND user_id=?",
			params![platform, user_id],
		)?;
		tx.commit()?;
		Ok(id)
	}
	pub fn complete_image_submission(
		&self,
		platform: &str,
		user_id: &str,
		request_id: &str,
		hash: &str,
	) -> Result<Option<String>> {
		ensure!(images::valid_hash(hash), "Invalid saved image hash");
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let current: Option<String> = tx
			.query_row(
				"SELECT id FROM image_requests WHERE platform=? AND user_id=?",
				params![platform, user_id],
				|r| r.get(0),
			)
			.optional()?;
		if current.as_deref() != Some(request_id) {
			return Ok(None);
		}
		ensure_image_customization(&tx, platform, user_id)?;
		tx.execute("INSERT INTO image_submissions(id,platform,user_id,image_hash,submitted_at) VALUES (?,?,?,?,?) ON CONFLICT(platform,user_id) DO UPDATE SET id=excluded.id,image_hash=excluded.image_hash,submitted_at=excluded.submitted_at", params![request_id, platform, user_id, hash, now_ms()])?;
		tx.execute("DELETE FROM image_requests WHERE id=?", [request_id])?;
		tx.commit()?;
		Ok(Some(request_id.to_owned()))
	}
	/// Already-saved bytes only; network callers must use the two-phase API.
	pub fn submit_image(&self, platform: &str, user_id: &str, hash: &str) -> Result<String> {
		ensure!(images::valid_hash(hash), "Invalid saved image hash");
		let id = self.begin_image_submission(platform, user_id)?;
		self
			.complete_image_submission(platform, user_id, &id, hash)?
			.context("Submission was superseded")
	}
	pub fn is_approved_image(&self, hash: &str) -> Result<bool> {
		if !images::valid_hash(hash) {
			return Ok(false);
		}
		Ok(self.db()?.query_row(
			"SELECT EXISTS(SELECT 1 FROM chatters WHERE image_hash=?)",
			[hash],
			|r| r.get(0),
		)?)
	}
	pub fn pending_images(&self) -> Result<Vec<ImageSubmission>> {
		let db = self.db()?;
		let mut stmt = db.prepare("SELECT s.id,s.platform,s.user_id,c.display_name,s.image_hash,s.submitted_at FROM image_submissions s JOIN chatters c USING(platform,user_id) ORDER BY s.submitted_at")?;
		Ok(stmt
			.query_map([], |r| {
				Ok(ImageSubmission {
					id: r.get(0)?,
					platform: r.get(1)?,
					user_id: r.get(2)?,
					display_name: r.get(3)?,
					image_hash: r.get(4)?,
					submitted_at: r.get(5)?,
				})
			})?
			.collect::<rusqlite::Result<Vec<_>>>()?)
	}
	pub fn review_image(&self, id: &str, approve: bool) -> Result<Chatter> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let row: Option<(String, String, String)> = tx
			.query_row(
				"SELECT platform,user_id,image_hash FROM image_submissions WHERE id=?",
				[id],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
			)
			.optional()?;
		let (platform, user, hash) = row.context("Submission was already reviewed or replaced")?;
		if approve {
			ensure_image_customization(&tx, &platform, &user)?;
			tx.execute(
				"UPDATE chatters SET image_hash=? WHERE platform=? AND user_id=?",
				params![hash, platform, user],
			)?;
		}
		tx.execute("DELETE FROM image_submissions WHERE id=?", [id])?;
		let chatter = tx.query_row(
			"SELECT * FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user],
			read_chatter,
		)?;
		tx.commit()?;
		Ok(chatter)
	}
	pub fn claim_message(&self, platform: &str, message_id: &str) -> Result<bool> {
		ensure!(
			!message_id.is_empty() && message_id.len() <= 512,
			"Message ID required and must be at most 512 bytes"
		);
		let db = self.db()?;
		db.execute(
			"DELETE FROM processed_messages WHERE processed_at<?",
			[now_ms() - 86_400_000],
		)?;
		Ok(db.execute(
			"INSERT OR IGNORE INTO processed_messages VALUES (?,?,?)",
			params![platform, message_id, now_ms()],
		)? == 1)
	}
	pub fn claim_command(&self, platform: &str, user_id: &str, now: i64) -> Result<bool> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let previous: Option<i64> = tx
			.query_row(
				"SELECT updated_at FROM command_cooldowns WHERE platform=? AND user_id=?",
				params![platform, user_id],
				|r| r.get(0),
			)
			.optional()?;
		// A clock correction must not lock a viewer out indefinitely.
		if previous.is_some_and(|p| now >= p && now.saturating_sub(p) < 5000) {
			return Ok(false);
		}
		tx.execute("INSERT INTO command_cooldowns VALUES (?,?,?) ON CONFLICT(platform,user_id) DO UPDATE SET updated_at=excluded.updated_at", params![platform, user_id, now])?;
		tx.commit()?;
		Ok(true)
	}
	pub fn create_turn(&self, id: &str, actor: &str, checkpoint: &serde_json::Value) -> Result<()> {
		self.db()?.execute(
			"INSERT INTO agent_turns VALUES (?,?,'running',?,?)",
			params![id, actor, checkpoint.to_string(), now_ms()],
		)?;
		Ok(())
	}
	pub fn checkpoint_turn(
		&self,
		id: &str,
		state: &str,
		checkpoint: &serde_json::Value,
	) -> Result<()> {
		ensure!(
            self.db()?.execute(
                "UPDATE agent_turns SET state=?,checkpoint=?,updated_at=? WHERE id=? AND state NOT IN ('cancelled','completed')",
                params![state, checkpoint.to_string(), now_ms(), id]
            )? == 1,
            "Agent turn does not exist or is already terminal"
        );
		Ok(())
	}
	/// Inspect a durable receipt without dispatching the operation again.
	pub fn saved_call(
		&self,
		turn: &str,
		call: &str,
		name: &str,
		args: &str,
	) -> Result<Option<(String, Option<String>)>> {
		let args = canonical_args(args)?;
		let saved: Option<(String, String, String, Option<String>)> = self
			.db()?
			.query_row(
				"SELECT name,args,state,result FROM tool_calls WHERE turn_id=? AND call_id=?",
				params![turn, call],
				|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
			)
			.optional()?;
		saved
			.map(|(old_name, old_args, state, result)| {
				ensure!(
					old_name == name && old_args == args,
					"Tool call identity reused with different arguments"
				);
				Ok((state, result))
			})
			.transpose()
	}
	pub fn begin_call(
		&self,
		turn: &str,
		call: &str,
		name: &str,
		args: &str,
		effect: CallEffect,
	) -> Result<Option<String>> {
		let args = canonical_args(args)?;
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let existing: Option<(String, String, String, Option<String>)> = tx
			.query_row(
				"SELECT name,args,state,result FROM tool_calls WHERE turn_id=? AND call_id=?",
				params![turn, call],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
			)
			.optional()?;
		if let Some((old_name, old_args, state, result)) = existing {
			ensure!(
				old_name == name && old_args == args,
				"Tool call identity reused with different arguments"
			);
			if state == "completed" {
				return Ok(Some(
					result.context("Completed action is missing its observed result")?,
				));
			}
			bail!("Tool action has an uncertain outcome; inspect its state before retrying");
		}
		let active: bool = tx.query_row(
			"SELECT EXISTS(SELECT 1 FROM agent_turns WHERE id=? AND state='running')",
			[turn],
			|r| r.get(0),
		)?;
		ensure!(active, "This turn is no longer running");
		if effect == CallEffect::MayMutate {
			// A new call ID, reordered JSON or changed audit reason does not erase
			// the same unresolved provider effect on the same resource. Pure reads
			// have no such effect: a lost read result must not block future inspection.
			let identity = uncertain_identity(name, &serde_json::from_str(&args)?);
			// These two creations are owned by the originating requester. A lost
			// receipt for Alice's memory/reminder cannot be Bob's repeated effect.
			// Exact-ID edits, shared settings, delivery and platform writes remain
			// global: another actor must not bypass an unresolved shared action.
			let actor: Option<String> = if matches!(name, "rememberMemory" | "createReminder") {
				Some(
					tx.query_row("SELECT actor FROM agent_turns WHERE id=?", [turn], |r| {
						r.get(0)
					})?,
				)
			} else {
				None
			};
			let mut query = tx.prepare(
				"SELECT c.args FROM tool_calls c LEFT JOIN agent_turns t ON t.id=c.turn_id WHERE c.name=? AND c.state IN ('started','unknown') AND (? IS NULL OR t.actor=? OR t.actor IS NULL)",
			)?;
			let prior = query
				.query_map(params![name, actor, actor], |r| r.get::<_, String>(0))?
				.collect::<rusqlite::Result<Vec<_>>>()?;
			let uncertain = prior.iter().any(|old| {
				serde_json::from_str::<serde_json::Value>(old)
					.is_ok_and(|old| uncertain_identity(name, &old) == identity)
			});
			drop(query);
			ensure!(
				!uncertain,
				"An equivalent tool action has an uncertain outcome; inspect its state before retrying"
			);
		}
		tx.execute(
			"INSERT INTO tool_calls(turn_id,call_id,name,args,state) VALUES (?,?,?,?,'started')",
			params![turn, call, name, args],
		)?;
		tx.commit()?;
		Ok(None)
	}
	pub fn finish_call(&self, turn: &str, call: &str, result: &str) -> Result<()> {
		let changed = self.db()?.execute("UPDATE tool_calls SET state='completed',result=? WHERE turn_id=? AND call_id=? AND state='started'", params![result, turn, call])?;
		ensure!(changed == 1, "Tool call is not running");
		Ok(())
	}
	/// Cancellation after dispatch must preserve the uncertain external effect.
	pub fn mark_call_uncertain(&self, turn: &str, call: &str) -> Result<()> {
		self.db()?.execute(
			"UPDATE tool_calls SET state='unknown' WHERE turn_id=? AND call_id=? AND state='started'",
			params![turn, call],
		)?;
		Ok(())
	}
	pub fn record_unknown_call(&self, turn: &str, call: &str, result: &str) -> Result<()> {
		ensure!(self.db()?.execute("UPDATE tool_calls SET state='unknown',result=? WHERE turn_id=? AND call_id=? AND state='started'", params![result,turn,call])?==1,"Tool call is not running");
		Ok(())
	}
	pub fn recover_interrupted(&self) -> Result<usize> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute(
			"UPDATE tool_calls SET state='unknown' WHERE state='started'",
			[],
		)?;
		tx.execute(
			"UPDATE agent_turns SET state='unknown' WHERE state='delivering'",
			[],
		)?;
		let changed = tx.execute(
			"UPDATE agent_turns SET state='interrupted' WHERE state IN ('running','queued_recovery')",
			[],
		)?;
		tx.commit()?;
		Ok(changed)
	}
}
fn uncertain_identity(name: &str, args: &serde_json::Value) -> serde_json::Value {
	let fields: &[&str] = match name {
		"banDiscordUser"
		| "timeoutDiscordUser"
		| "disconnectDiscordVoiceUser"
		| "moveDiscordVoiceUser"
		| "setDiscordNickname" => &["guildId", "userId"],
		"discordManageRole" => &["guildId", "userId", "roleId", "action"],
		"discordStageControlUser" => &["guildId", "userId", "action"],
		"editDiscordChannel" | "deleteDiscordChannel" => &["guildId", "channelId"],
		"deleteDiscordMessage" | "pinDiscordMessage" | "unpinDiscordMessage" => {
			&["guildId", "channelId", "messageId"]
		}
		"createDiscordChannel" => &["guildId", "parentId", "name"],
		"createDiscordThread" => &["guildId", "channelId", "messageId", "name"],
		"banTwitchUser" => &["broadcasterId", "userId"],
		"startTwitchRaid"
		| "cancelTwitchRaid"
		| "startTwitchCommercial"
		| "setTwitchTitle"
		| "setTwitchCategory"
		| "setTwitchTags"
		| "createTwitchMarker"
		| "createTwitchClip"
		| "updateTwitchChatSettings" => &["broadcasterId"],
		"twitchShoutout" => &["broadcasterId", "targetBroadcasterId"],
		"banYoutubeUser" => &["liveChatId", "userId"],
		"deleteYoutubeMessage" => &["liveChatId", "messageId"],
		"createPoll" => &["platform", "channelId"],
		"closePoll" => &["platform", "channelId", "pollId"],
		_ => return args.clone(),
	};
	serde_json::Value::Object(
		fields
			.iter()
			.map(|key| (key.to_string(), args[*key].clone()))
			.collect(),
	)
}
fn clear_image_requests(db: &Connection, platform: &str, user_id: &str) -> Result<()> {
	db.execute(
		"DELETE FROM image_submissions WHERE platform=? AND user_id=?",
		params![platform, user_id],
	)?;
	db.execute(
		"DELETE FROM image_requests WHERE platform=? AND user_id=?",
		params![platform, user_id],
	)?;
	Ok(())
}
fn ensure_image_customization(db: &Connection, platform: &str, user_id: &str) -> Result<()> {
	let blocked: bool = db
		.query_row(
			"SELECT customization_blocked FROM chatters WHERE platform=? AND user_id=?",
			params![platform, user_id],
			|r| r.get(0),
		)
		.context("Profile does not exist")?;
	ensure!(
		!blocked,
		"Customization is disabled for your profile by the streamer"
	);
	let settings: Option<String> = db
		.query_row(
			"SELECT value FROM settings WHERE key='installation'",
			[],
			|r| r.get(0),
		)
		.optional()?;
	let settings: Settings = settings
		.map(|json| serde_json::from_str(&json))
		.transpose()?
		.unwrap_or_default();
	ensure!(
		settings.custom_images_enabled,
		"Custom image submissions are disabled on this stream"
	);
	Ok(())
}
fn canonical_args(args: &str) -> Result<String> {
	fn sort(value: serde_json::Value) -> serde_json::Value {
		match value {
			serde_json::Value::Object(map) => serde_json::Value::Object(
				map.into_iter()
					.map(|(key, value)| (key, sort(value)))
					.collect::<std::collections::BTreeMap<_, _>>()
					.into_iter()
					.collect(),
			),
			serde_json::Value::Array(values) => {
				serde_json::Value::Array(values.into_iter().map(sort).collect())
			}
			value => value,
		}
	}
	Ok(serde_json::to_string(&sort(
		serde_json::from_str(args).context("Tool arguments must be valid JSON")?,
	))?)
}
fn read_chatter(r: &rusqlite::Row<'_>) -> rusqlite::Result<Chatter> {
	Ok(Chatter {
		platform: r.get(0)?,
		user_id: r.get(1)?,
		display_name: r.get(2)?,
		puppet_id: r.get(3)?,
		voice_id: r.get(4)?,
		image_hash: r.get(5)?,
		customization_blocked: r.get(6)?,
		overrides: serde_json::from_str(&r.get::<_, String>(7)?).map_err(|e| {
			rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
		})?,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	fn hash(c: char) -> String {
		std::iter::repeat_n(c, 64).collect()
	}
	fn store() -> Store {
		Store::open(Path::new(":memory:")).unwrap()
	}
	#[test]
	fn chatter_persists_and_display_name_changes_without_reroll() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		let first = {
			let s = Store::open(&path).unwrap();
			s.ensure_chatter("twitch", "123", "Old").unwrap()
		};
		let s = Store::open(&path).unwrap();
		let second = s.ensure_chatter("twitch", "123", "New").unwrap();
		assert_eq!(first.puppet_id, second.puppet_id);
		assert_eq!(first.voice_id, second.voice_id);
		assert_eq!(second.display_name, "New");
		s.ensure_chatter("discord", "123", "New").unwrap();
		assert_eq!(s.chatters("").unwrap().len(), 2);
		assert_eq!(s.chatters("123").unwrap().len(), 2);
	}
	#[test]
	fn migration_preserves_profiles_and_future_schema_is_not_downgraded() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		let first = {
			let s = Store::open(&path).unwrap();
			s.ensure_chatter("twitch", "1", "Viewer").unwrap()
		};
		let db = Connection::open(&path).unwrap();
		db.execute_batch(
			"ALTER TABLE chatters DROP COLUMN overrides; DROP TABLE image_requests; DROP INDEX tool_calls_action_state; DROP TABLE pending_inputs; DROP TABLE conversation_history; ALTER TABLE memories DROP COLUMN actor; DROP INDEX reminder_due; ALTER TABLE reminders DROP COLUMN actor; ALTER TABLE reminders DROP COLUMN destination; ALTER TABLE reminders DROP COLUMN state; ALTER TABLE reminders DROP COLUMN receipt; PRAGMA user_version=1;",
		)
		.unwrap();
		let s = Store::open(&path).unwrap();
		assert_eq!(s.chatter("twitch", "1").unwrap().voice_id, first.voice_id);
		drop(s);
		assert_eq!(
			db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
				.unwrap(),
			SCHEMA_VERSION
		);
		db.pragma_update(None, "user_version", 999).unwrap();
		assert!(Store::open(&path).is_err());
		assert_eq!(
			db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
				.unwrap(),
			999
		);
	}
	#[test]
	fn approval_cannot_accept_a_superseded_image_and_network_order_cannot_restore_it() {
		let s = store();
		let initial = s.ensure_chatter("twitch", "1", "Viewer").unwrap();
		let old = s.submit_image("twitch", "1", &hash('a')).unwrap();
		let slow = s.begin_image_submission("twitch", "1").unwrap();
		assert!(s.review_image(&old, true).is_err());
		let latest = s.begin_image_submission("twitch", "1").unwrap();
		let submitted = s
			.complete_image_submission("twitch", "1", &latest, &hash('b'))
			.unwrap()
			.unwrap();
		assert!(
			s.complete_image_submission("twitch", "1", &slow, &hash('c'))
				.unwrap()
				.is_none()
		);
		assert_eq!(
			s.chatter("twitch", "1").unwrap().puppet_id,
			initial.puppet_id
		);
		assert_eq!(s.chatter("twitch", "1").unwrap().image_hash, None);
		assert_eq!(
			s.review_image(&submitted, true)
				.unwrap()
				.image_hash
				.as_deref(),
			Some(hash('b').as_str())
		);
		assert!(s.review_image(&submitted, true).is_err());
	}
	#[test]
	fn block_reset_and_builtin_selection_invalidate_inflight_images() {
		let s = store();
		s.ensure_chatter("twitch", "1", "Viewer").unwrap();
		let pending = s.submit_image("twitch", "1", &hash('a')).unwrap();
		s.review_image(&pending, true).unwrap();
		let inflight = s.begin_image_submission("twitch", "1").unwrap();
		s.block_customization("twitch", "1", true).unwrap();
		assert!(
			s.complete_image_submission("twitch", "1", &inflight, &hash('b'))
				.unwrap()
				.is_none()
		);
		assert!(s.select_puppet("twitch", "1", "dandy").is_err());
		assert!(s.select_voice("twitch", "1", "alopex").is_err());
		assert!(s.submit_image("twitch", "1", &hash('b')).is_err());
		let moderated = s.set_chatter_puppet("twitch", "1", "dandy").unwrap();
		assert!(moderated.customization_blocked && moderated.image_hash.is_none());
		assert_eq!(
			s.set_chatter_voice("twitch", "1", "alopex")
				.unwrap()
				.voice_id,
			"alopex"
		);
		assert!(
			s.set_chatter_voice("twitch", "1", "bumblebee-buddy")
				.is_err()
		);
		let reset = s.reset_chatter("twitch", "1").unwrap();
		assert!(reset.customization_blocked);
		assert!(reset.image_hash.is_none());
		s.block_customization("twitch", "1", false).unwrap();
		let inflight = s.begin_image_submission("twitch", "1").unwrap();
		s.select_puppet("twitch", "1", "dandy").unwrap();
		assert!(
			s.complete_image_submission("twitch", "1", &inflight, &hash('b'))
				.unwrap()
				.is_none()
		);
	}
	#[test]
	fn interrupted_actions_never_automatically_reexecute_under_new_ids_or_key_order() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		{
			let s = Store::open(&path).unwrap();
			s.create_turn("t", "owner", &serde_json::json!({})).unwrap();
			assert!(
				s.begin_call(
					"t",
					"call",
					"ban",
					r#"{"user":"123","scope":{"b":2,"a":1}}"#,
					CallEffect::MayMutate
				)
				.unwrap()
				.is_none()
			);
		}
		let s = Store::open(&path).unwrap();
		s.recover_interrupted().unwrap();
		assert!(
			s.begin_call(
				"t",
				"call",
				"ban",
				r#"{"scope":{"a":1,"b":2},"user":"123"}"#,
				CallEffect::MayMutate
			)
			.is_err()
		);
		s.create_turn("new", "owner", &serde_json::json!({}))
			.unwrap();
		assert!(
			s.begin_call(
				"new",
				"different-call",
				"ban",
				r#"{"scope":{"a":1,"b":2},"user":"123"}"#,
				CallEffect::MayMutate
			)
			.is_err()
		);
		assert!(
			s.begin_call(
				"new",
				"other-user",
				"ban",
				r#"{"scope":{"a":1,"b":2},"user":"456"}"#,
				CallEffect::MayMutate
			)
			.unwrap()
			.is_none()
		);
	}
	#[test]
	fn completed_calls_reuse_observed_result_and_reject_identity_reuse() {
		let s = store();
		s.create_turn("t", "owner", &serde_json::json!({})).unwrap();
		s.begin_call("t", "c", "read", "{}", CallEffect::ReadOnly)
			.unwrap();
		s.finish_call("t", "c", "result").unwrap();
		assert_eq!(
			s.begin_call("t", "c", "read", "{}", CallEffect::ReadOnly)
				.unwrap()
				.as_deref(),
			Some("result")
		);
		assert!(
			s.begin_call("t", "c", "write", "{}", CallEffect::MayMutate)
				.is_err()
		);
	}
	#[test]
	fn interrupted_reads_allow_fresh_inspection_without_replaying_unknown_local_writes() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		{
			let s = Store::open(&path).unwrap();
			s.create_turn("old", "discord:alice", &serde_json::json!({}))
				.unwrap();
			s.begin_call("old", "read", "listMemories", "{}", CallEffect::ReadOnly)
				.unwrap();
			s.begin_call(
				"old",
				"write",
				"rememberMemory",
				r#"{"content":"a durable fact"}"#,
				CallEffect::MayMutate,
			)
			.unwrap();
			// Simulate termination before either result is saved.
		}
		let s = Store::open(&path).unwrap();
		s.recover_interrupted().unwrap();
		// A read's original call identity still cannot silently execute twice.
		assert!(
			s.begin_call("old", "read", "listMemories", "{}", CallEffect::ReadOnly)
				.is_err()
		);
		// Acknowledging the interrupted turn must neither poison every future read
		// nor erase the unresolved mutation.
		s.cancel_agent_turn("old").unwrap();
		s.create_turn("new", "discord:bob", &serde_json::json!({}))
			.unwrap();
		assert!(
			s.begin_call("new", "inspect", "listMemories", "{}", CallEffect::ReadOnly)
				.unwrap()
				.is_none()
		);
		s.finish_call("new", "inspect", "[]").unwrap();
		assert_eq!(
			s.begin_call("new", "inspect", "listMemories", "{}", CallEffect::ReadOnly)
				.unwrap()
				.as_deref(),
			Some("[]")
		);
		s.create_turn("alice-retry", "discord:alice", &serde_json::json!({}))
			.unwrap();
		assert!(
			s.begin_call(
				"alice-retry",
				"retry",
				"rememberMemory",
				r#"{"content":"a durable fact"}"#,
				CallEffect::MayMutate
			)
			.is_err()
		);
	}
	#[test]
	fn uncertain_requester_owned_creations_do_not_poison_other_requesters_or_weaken_shared_writes() {
		let dir = tempfile::tempdir().unwrap();
		let s = Store::open(&dir.path().join("test.sqlite")).unwrap();
		for (turn, actor) in [
			("a", "discord:alice"),
			("a2", "discord:alice"),
			("b", "discord:bob"),
		] {
			s.create_turn(turn, actor, &serde_json::json!({})).unwrap();
		}
		for (name, args) in [
			("rememberMemory", r#"{"content":"I like blue"}"#),
			(
				"createReminder",
				r#"{"content":"stretch","dueAt":"2026-10-09T12:00:00Z"}"#,
			),
		] {
			s.begin_call("a", name, name, args, CallEffect::MayMutate)
				.unwrap();
			s.mark_call_uncertain("a", name).unwrap();
			assert!(
				s.begin_call("a2", name, name, args, CallEffect::MayMutate)
					.is_err()
			);
			assert_eq!(
				s.begin_call("b", name, name, args, CallEffect::MayMutate)
					.unwrap(),
				None
			);
		}
		for (name, args) in [
			(
				"setTwitchTitle",
				r#"{"broadcasterId":"123","title":"same"}"#,
			),
			("updateMemory", r#"{"id":"shared-id","content":"same"}"#),
			("updateSettings", r#"{"masterVolume":0.5}"#),
		] {
			s.begin_call("a", name, name, args, CallEffect::MayMutate)
				.unwrap();
			s.mark_call_uncertain("a", name).unwrap();
			assert!(
				s.begin_call("b", name, name, args, CallEffect::MayMutate)
					.is_err(),
				"shared action {name} must retain the global barrier"
			);
		}
	}
	#[test]
	fn recovery_reads_exact_saved_receipts_and_reclaims_interrupted_queue_entries() {
		let s = store();
		s.create_turn("turn", "owner", &serde_json::json!({}))
			.unwrap();
		s.begin_call(
			"turn",
			"done",
			"write",
			r#"{"b":2,"a":1}"#,
			CallEffect::MayMutate,
		)
		.unwrap();
		s.finish_call("turn", "done", r#"{"status":"verified"}"#)
			.unwrap();
		s.begin_call("turn", "lost", "other", "{}", CallEffect::MayMutate)
			.unwrap();
		s.checkpoint_turn("turn", "queued_recovery", &serde_json::json!({}))
			.unwrap();
		s.recover_interrupted().unwrap();
		assert_eq!(s.turn("turn").unwrap().state, "interrupted");
		assert_eq!(
			s.saved_call("turn", "done", "write", r#"{"a":1,"b":2}"#)
				.unwrap(),
			Some(("completed".into(), Some(r#"{"status":"verified"}"#.into())))
		);
		assert_eq!(
			s.saved_call("turn", "lost", "other", "{}").unwrap(),
			Some(("unknown".into(), None))
		);
		assert!(s.saved_call("turn", "done", "write", "{}").is_err());
		assert!(
			s.saved_call("turn", "done", "other", r#"{"a":1,"b":2}"#)
				.is_err()
		);
		assert_eq!(s.saved_call("turn", "absent", "other", "{}").unwrap(), None);
	}
	#[test]
	fn provider_deduplication_survives_restart_and_is_platform_scoped() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		{
			let s = Store::open(&path).unwrap();
			assert!(s.claim_message("twitch", "id").unwrap());
		}
		let s = Store::open(&path).unwrap();
		assert!(!s.claim_message("twitch", "id").unwrap());
		assert!(s.claim_message("youtube", "id").unwrap());
		assert!(s.claim_command("twitch", "1", 10000).unwrap());
		assert!(!s.claim_command("twitch", "1", 10001).unwrap());
		assert!(s.claim_command("twitch", "1", 15000).unwrap());
		assert!(s.claim_command("twitch", "1", 5000).unwrap());
	}
	#[test]
	fn pending_and_rejected_image_bytes_are_not_obs_assets() {
		let s = store();
		s.ensure_chatter("twitch", "1", "Viewer").unwrap();
		let image = hash('a');
		let id = s.submit_image("twitch", "1", &image).unwrap();
		assert!(!s.is_approved_image(&image).unwrap());
		s.review_image(&id, false).unwrap();
		assert!(!s.is_approved_image(&image).unwrap());
		let id = s.submit_image("twitch", "1", &image).unwrap();
		s.review_image(&id, true).unwrap();
		assert!(s.is_approved_image(&image).unwrap());
		s.reset_chatter("twitch", "1").unwrap();
		assert!(!s.is_approved_image(&image).unwrap());
	}
	#[test]
	fn changed_moderation_reason_does_not_bypass_an_uncertain_target() {
		let s = store();
		s.create_turn("first", "owner", &serde_json::json!({}))
			.unwrap();
		s.begin_call(
			"first",
			"c1",
			"banDiscordUser",
			r#"{"guildId":"1","userId":"2","reason":"spam"}"#,
			CallEffect::MayMutate,
		)
		.unwrap();
		s.recover_interrupted().unwrap();
		s.create_turn("second", "owner", &serde_json::json!({}))
			.unwrap();
		assert!(
			s.begin_call(
				"second",
				"c2",
				"banDiscordUser",
				r#"{"guildId":"1","userId":"2","reason":"retry with other text"}"#,
				CallEffect::MayMutate
			)
			.is_err()
		);
		assert!(
			s.begin_call(
				"second",
				"c3",
				"banDiscordUser",
				r#"{"guildId":"1","userId":"3","reason":"spam"}"#,
				CallEffect::MayMutate
			)
			.is_ok()
		);
	}
	#[test]
	fn cancelled_turns_cannot_be_resurrected_by_late_checkpoints() {
		let s = store();
		s.create_turn("turn", "owner", &serde_json::json!({}))
			.unwrap();
		s.cancel_agent_turn("turn").unwrap();
		assert!(
			s.checkpoint_turn("turn", "running", &serde_json::json!({}))
				.is_err()
		);
		assert!(
			s.begin_call("turn", "new", "write", "{}", CallEffect::MayMutate)
				.is_err()
		);
	}
}

#[cfg(test)]
mod preference_migration_tests {
	use super::*;
	#[test]
	fn first_preview_settings_layout_and_undo_migrate_without_losing_profiles() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("app.db");
		let store = Store::open(&path).unwrap();
		let profile = store.ensure_chatter("twitch", "1", "Viewer").unwrap();
		let settings = serde_json::json!({"ownerDiscordId":"123","twitchChannel":"chevcast","aiEnabled":true,"overlayPort":2999});
		let layout = serde_json::json!({"beeX":0.7,"beeY":0.4,"beeScale":0.3,"beeVisible":false,"puppetScale":0.2,"puppetHorizontal":0.6,"puppetOcclusion":0.2,"puppetsVisible":true,"bubblesVisible":false});
		store.set("installation", &settings).unwrap();
		store.set("overlay_settings", &layout).unwrap();
		store
			.set(
				"settings_undo",
				&serde_json::json!({"key":"overlay_settings","before":layout,"after":layout}),
			)
			.unwrap();
		drop(store);
		let db = Connection::open(&path).unwrap();
		db.execute_batch("ALTER TABLE chatters DROP COLUMN overrides; PRAGMA user_version=3;")
			.unwrap();
		drop(db);
		let store = Store::open(&path).unwrap();
		let s = store.settings().unwrap();
		assert_eq!(s.owner_discord_id, "123");
		assert_eq!(s.overlay_port, 2999);
		assert_eq!(s.twitch_channel, "chevcast");
		assert_eq!(
			store.chatter("twitch", "1").unwrap().voice_id,
			profile.voice_id
		);
		let overlay: OverlaySettings = store.get("overlay_settings").unwrap().unwrap();
		assert_eq!(overlay.bumblebee.position.horizontal_percent, 70.);
		assert!(!overlay.bumblebee.visible);
		let undo: serde_json::Value = store.get("settings_undo").unwrap().unwrap();
		assert_eq!(undo["before"]["bumblebee"]["visible"], false);
		store
			.patch_settings(&serde_json::json!({"masterVolume":0.4}))
			.unwrap();
		store
			.patch_settings(&serde_json::json!({"chatPlatforms":{"youtube":{"relay":true}}}))
			.unwrap();
		store
			.patch_overlay(&serde_json::json!({"streamerVoiceBubble":{"enabled":true}}))
			.unwrap();
		drop(store);
		let store = Store::open(&path).unwrap();
		assert_eq!(store.settings().unwrap().master_volume, 0.4);
		assert!(store.settings().unwrap().chat_platforms.youtube.relay);
		assert_eq!(store.settings().unwrap().owner_discord_id, "123");
		assert!(
			!store
				.get::<OverlaySettings>("overlay_settings")
				.unwrap()
				.unwrap()
				.bumblebee
				.visible
		);
	}
}
