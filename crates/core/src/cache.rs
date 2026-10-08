//! A disposable, bounded speech cache. Every entry stores audio and word timing
//! in the same SQLite row and transaction; no partially generated file is a hit.
//! Run these synchronous operations on the database/blocking worker, not the audio clock.

use crate::{images::valid_hash, model::WordTiming};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{
	path::Path,
	sync::{Mutex, MutexGuard},
};

pub const SPEECH_CACHE_TTL_MS: i64 = 3 * 24 * 60 * 60 * 1000;
const MAX_ENTRIES: i64 = 4096;
const MAX_TIMING_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CachedSpeech {
	pub audio: Vec<u8>,
	pub words: Vec<WordTiming>,
	pub content_type: String,
}

pub struct SpeechCache {
	connection: Mutex<Connection>,
	max_bytes: i64,
}

impl SpeechCache {
	/// max_bytes bounds entry payloads plus their metadata. SQLite has a small
	/// additional page/index overhead; FULL auto-vacuum reclaims evicted pages.
	pub fn open(directory: &Path, max_bytes: u64) -> Result<Self> {
		ensure!(
			max_bytes > 0 && max_bytes <= i64::MAX as u64,
			"Invalid speech cache size"
		);
		std::fs::create_dir_all(directory)?;
		let mut db = Connection::open(directory.join("speech-cache.sqlite"))?;
		db.busy_timeout(std::time::Duration::from_secs(5))?;
		let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
		ensure!(
			version <= 1,
			"Speech cache belongs to a newer application version"
		);
		// No persistent WAL or separate audio files can outgrow the byte budget.
		db.execute_batch(
			"PRAGMA journal_mode=DELETE; PRAGMA auto_vacuum=FULL; PRAGMA synchronous=NORMAL;",
		)?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		tx.execute_batch(
			"CREATE TABLE IF NOT EXISTS speech_entries (
            key TEXT PRIMARY KEY, audio BLOB NOT NULL, words TEXT NOT NULL,
            content_type TEXT NOT NULL, checksum BLOB NOT NULL,
            accessed_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
            bytes INTEGER NOT NULL CHECK(bytes>0));
            CREATE INDEX IF NOT EXISTS speech_expiry ON speech_entries(expires_at);
            CREATE INDEX IF NOT EXISTS speech_lru ON speech_entries(accessed_at,key);
            PRAGMA user_version=1;",
		)?;
		tx.commit()?;
		let cache = Self {
			connection: Mutex::new(db),
			max_bytes: max_bytes as i64,
		};
		cache.prune(crate::now_ms())?;
		Ok(cache)
	}

	fn db(&self) -> Result<MutexGuard<'_, Connection>> {
		self
			.connection
			.lock()
			.map_err(|_| anyhow::anyhow!("Speech cache lock poisoned"))
	}

	pub fn get(&self, key: &str, now_ms: i64) -> Result<Option<CachedSpeech>> {
		ensure!(valid_hash(key), "Speech cache keys must be SHA-256 hashes");
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		// Reject oversized or incomplete rows before allocating their contents.
		let size: Option<(i64, i64, i64, i64)> = tx
			.query_row(
				"SELECT expires_at,length(audio),length(words),bytes FROM speech_entries WHERE key=?",
				[key],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
			)
			.optional()?;
		let Some((expires, audio_len, words_len, bytes)) = size else {
			return Ok(None);
		};
		if expires <= now_ms
			|| audio_len <= 0
			|| audio_len > self.max_bytes
			|| words_len <= 2
			|| words_len > MAX_TIMING_BYTES as i64
			|| bytes > self.max_bytes
		{
			tx.execute("DELETE FROM speech_entries WHERE key=?", [key])?;
			tx.commit()?;
			return Ok(None);
		}
		let (audio, words_json, content_type, checksum): (Vec<u8>, String, String, Vec<u8>) = tx
			.query_row(
				"SELECT audio,words,content_type,checksum FROM speech_entries WHERE key=?",
				[key],
				|r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
			)?;
		let words = serde_json::from_str::<Vec<WordTiming>>(&words_json).ok();
		let entry = words.map(|words| CachedSpeech {
			audio,
			words,
			content_type,
		});
		let complete = entry.as_ref().is_some_and(|entry| {
			validate(entry).is_ok()
				&& entry_size(entry, &words_json) == bytes
				&& digest(entry, &words_json).as_slice() == checksum.as_slice()
		});
		if !complete {
			tx.execute("DELETE FROM speech_entries WHERE key=?", [key])?;
			tx.commit()?;
			return Ok(None);
		}
		tx.execute(
			"UPDATE speech_entries SET accessed_at=?,expires_at=? WHERE key=?",
			params![now_ms, now_ms.saturating_add(SPEECH_CACHE_TTL_MS), key],
		)?;
		tx.commit()?;
		Ok(entry)
	}

	/// Oversized complete results remain usable by the caller but are not cached.
	/// A failed/unfinished synthesis must never be passed to this function.
	pub fn put(&self, key: &str, entry: &CachedSpeech, now_ms: i64) -> Result<()> {
		ensure!(valid_hash(key), "Speech cache keys must be SHA-256 hashes");
		validate(entry)?;
		let words = serde_json::to_string(&entry.words)?;
		ensure!(
			words.len() <= MAX_TIMING_BYTES,
			"Speech timing metadata is too large"
		);
		let bytes = entry_size(entry, &words);
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		if bytes <= self.max_bytes {
			tx.execute("INSERT INTO speech_entries(key,audio,words,content_type,checksum,accessed_at,expires_at,bytes)
                VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(key) DO UPDATE SET
                audio=excluded.audio,words=excluded.words,content_type=excluded.content_type,
                checksum=excluded.checksum,accessed_at=excluded.accessed_at,
                expires_at=excluded.expires_at,bytes=excluded.bytes",
                params![key, entry.audio, words, entry.content_type, digest(entry, &words).as_slice(), now_ms, now_ms.saturating_add(SPEECH_CACHE_TTL_MS), bytes])?;
		} else {
			tx.execute("DELETE FROM speech_entries WHERE key=?", [key])?;
		}
		prune_entries(&tx, now_ms, self.max_bytes)?;
		tx.commit()?;
		Ok(())
	}

	pub fn prune(&self, now_ms: i64) -> Result<usize> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let removed = prune_entries(&tx, now_ms, self.max_bytes)?;
		tx.commit()?;
		Ok(removed)
	}
}

fn prune_entries(db: &Connection, now_ms: i64, max_bytes: i64) -> Result<usize> {
	let expired = db.execute("DELETE FROM speech_entries WHERE expires_at<=?", [now_ms])?;
	let evicted = db.execute(
		"DELETE FROM speech_entries WHERE key IN (
        SELECT key FROM (
            SELECT key,SUM(bytes) OVER (ORDER BY accessed_at DESC,key) AS retained_bytes,
            ROW_NUMBER() OVER (ORDER BY accessed_at DESC,key) AS recency
            FROM speech_entries
        ) WHERE retained_bytes>? OR recency>?
    )",
		params![max_bytes, MAX_ENTRIES],
	)?;
	Ok(expired + evicted)
}

fn validate(entry: &CachedSpeech) -> Result<()> {
	ensure!(
		!entry.audio.is_empty() && !entry.words.is_empty(),
		"Complete speech audio and timing are required"
	);
	ensure!(
		entry.content_type.starts_with("audio/")
			&& entry.content_type.len() <= 128
			&& entry
				.content_type
				.bytes()
				.all(|c| c.is_ascii_graphic() || c == b' '),
		"Invalid audio content type"
	);
	let mut last = 0;
	for word in &entry.words {
		ensure!(
			!word.text.is_empty() && word.start_ms >= last,
			"Invalid speech word timing"
		);
		word
			.start_ms
			.checked_add(word.duration_ms)
			.context("Speech word timing overflows")?;
		last = word.start_ms;
	}
	Ok(())
}
fn entry_size(entry: &CachedSpeech, words: &str) -> i64 {
	// Include the key, checksum, timestamps, size and row metadata allowance.
	entry
		.audio
		.len()
		.saturating_add(words.len())
		.saturating_add(entry.content_type.len())
		.saturating_add(160)
		.try_into()
		.unwrap_or(i64::MAX)
}
fn digest(entry: &CachedSpeech, words: &str) -> Vec<u8> {
	let mut hash = Sha256::new();
	hash.update((entry.content_type.len() as u64).to_le_bytes());
	hash.update(entry.content_type.as_bytes());
	hash.update((words.len() as u64).to_le_bytes());
	hash.update(words.as_bytes());
	hash.update(&entry.audio);
	hash.finalize().to_vec()
}

#[cfg(test)]
mod tests {
	use super::*;
	fn key(text: &str) -> String {
		format!("{:x}", Sha256::digest(text))
	}
	fn entry(value: u8) -> CachedSpeech {
		CachedSpeech {
			audio: vec![value; 1024],
			words: vec![WordTiming {
				text: "hello".into(),
				start_ms: 0,
				duration_ms: 100,
			}],
			content_type: "audio/wav".into(),
		}
	}
	#[test]
	fn complete_speech_survives_restart_and_hits_slide_expiration_by_three_days() {
		let dir = tempfile::tempdir().unwrap();
		let now = crate::now_ms();
		let id = key("speech");
		{
			let cache = SpeechCache::open(dir.path(), 100_000).unwrap();
			cache.put(&id, &entry(1), now).unwrap();
		}
		let cache = SpeechCache::open(dir.path(), 100_000).unwrap();
		let hit_at = now + SPEECH_CACHE_TTL_MS - 1;
		let hit = cache.get(&id, hit_at).unwrap().unwrap();
		assert_eq!(hit.audio, entry(1).audio);
		assert_eq!(hit.words[0].duration_ms, 100);
		assert!(
			cache
				.get(&id, hit_at + SPEECH_CACHE_TTL_MS - 1)
				.unwrap()
				.is_some()
		);
		assert!(
			cache
				.get(&id, hit_at + 2 * SPEECH_CACHE_TTL_MS - 1)
				.unwrap()
				.is_none()
		);
	}
	#[test]
	fn missing_timing_or_corrupt_audio_is_a_miss() {
		let dir = tempfile::tempdir().unwrap();
		let cache = SpeechCache::open(dir.path(), 100_000).unwrap();
		let now = crate::now_ms();
		for (id, update) in [
			(key("no timing"), "words='[]'"),
			(key("no audio"), "audio=x''"),
			(key("corrupt"), "audio=x'010203'"),
		] {
			cache.put(&id, &entry(1), now).unwrap();
			cache
				.db()
				.unwrap()
				.execute(
					&format!("UPDATE speech_entries SET {update} WHERE key=?"),
					[&id],
				)
				.unwrap();
			assert!(cache.get(&id, now).unwrap().is_none());
			assert_eq!(
				cache
					.db()
					.unwrap()
					.query_row(
						"SELECT COUNT(*) FROM speech_entries WHERE key=?",
						[&id],
						|r| r.get::<_, i64>(0)
					)
					.unwrap(),
				0
			);
		}
	}
	#[test]
	fn uncommitted_partial_replacement_never_becomes_a_hit() {
		let dir = tempfile::tempdir().unwrap();
		let now = crate::now_ms();
		let id = key("speech");
		{
			let cache = SpeechCache::open(dir.path(), 100_000).unwrap();
			cache.put(&id, &entry(1), now).unwrap();
			let db = cache.db().unwrap();
			db.execute_batch("BEGIN IMMEDIATE;").unwrap();
			db.execute(
				"UPDATE speech_entries SET audio=? WHERE key=?",
				params![entry(2).audio, id],
			)
			.unwrap();
			// Dropping the connection before COMMIT mirrors interrupted publication.
		}
		let cache = SpeechCache::open(dir.path(), 100_000).unwrap();
		assert_eq!(cache.get(&id, now).unwrap().unwrap().audio, entry(1).audio);
	}
	#[test]
	fn byte_budget_evicts_least_recently_used_complete_entries() {
		let dir = tempfile::tempdir().unwrap();
		let now = crate::now_ms();
		let speech = entry(1);
		let size = entry_size(&speech, &serde_json::to_string(&speech.words).unwrap());
		let cache = SpeechCache::open(dir.path(), (size * 2) as u64).unwrap();
		cache.put(&key("first"), &speech, now).unwrap();
		cache.put(&key("second"), &speech, now + 1).unwrap();
		cache.get(&key("first"), now + 2).unwrap().unwrap();
		cache.put(&key("third"), &speech, now + 3).unwrap();
		assert!(cache.get(&key("second"), now + 4).unwrap().is_none());
		assert!(cache.get(&key("first"), now + 4).unwrap().is_some());
		assert!(cache.get(&key("third"), now + 4).unwrap().is_some());
		let sum: i64 = cache
			.db()
			.unwrap()
			.query_row("SELECT SUM(bytes) FROM speech_entries", [], |r| r.get(0))
			.unwrap();
		assert!(sum <= size * 2);
		assert_eq!(cache.prune(now + 4 + SPEECH_CACHE_TTL_MS).unwrap(), 2);
	}
	#[test]
	fn incomplete_or_oversized_results_are_not_published() {
		let dir = tempfile::tempdir().unwrap();
		let cache = SpeechCache::open(dir.path(), 500).unwrap();
		let now = crate::now_ms();
		let id = key("speech");
		cache.put(&id, &entry(1), now).unwrap();
		assert!(cache.get(&id, now).unwrap().is_none());
		let mut incomplete = entry(1);
		incomplete.words.clear();
		assert!(cache.put(&id, &incomplete, now).is_err());
		assert!(cache.get(&id, now).unwrap().is_none());
	}
}
