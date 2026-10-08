//! Atomic preference changes shared by desktop section resets.
use crate::{
	model::{OverlaySettings, Settings},
	storage::Store,
};
use anyhow::Result;
use rusqlite::{OptionalExtension, TransactionBehavior};

impl Store {
	/// Keep preference and layout resets in one transaction; unrelated settings and user data stay intact.
	pub fn patch_preferences(
		&self,
		patch: &serde_json::Value,
		reset_overlay: bool,
	) -> Result<(Settings, Option<OverlaySettings>)> {
		let mut db = self.db()?;
		let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
		let old: Option<String> = tx
			.query_row(
				"SELECT value FROM settings WHERE key='installation'",
				[],
				|row| row.get(0),
			)
			.optional()?;
		let previous: Settings = old
			.map(|value| serde_json::from_str(&value))
			.transpose()?
			.unwrap_or_default();
		let next = previous.patched(patch)?;
		let overlay = reset_overlay.then(OverlaySettings::default);
		if let Some(overlay) = &overlay {
			overlay.validate()?;
		}
		tx.execute("INSERT INTO settings(key,value) VALUES ('installation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [serde_json::to_string(&next)?])?;
		if let Some(overlay) = &overlay {
			tx.execute("INSERT INTO settings(key,value) VALUES ('overlay_settings',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [serde_json::to_string(overlay)?])?;
		}
		tx.commit()?;
		Ok((next, overlay))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn failed_layout_write_rolls_back_preferences_too() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::open(&dir.path().join("reset.sqlite3")).unwrap();
		store
			.patch_settings(&serde_json::json!({"masterVolume":0.25}))
			.unwrap();
		store.db().unwrap().execute_batch("CREATE TRIGGER reject_overlay BEFORE INSERT ON settings WHEN NEW.key='overlay_settings' BEGIN SELECT RAISE(ABORT,'simulated disk write failure'); END;").unwrap();
		assert!(
			store
				.patch_preferences(&serde_json::json!({"masterVolume":1.0}), true)
				.is_err()
		);
		assert_eq!(store.settings().unwrap().master_volume, 0.25);
		assert!(
			store
				.get::<OverlaySettings>("overlay_settings")
				.unwrap()
				.is_none()
		);
	}
	#[test]
	fn sparse_reset_reads_latest_settings_and_rejects_invalid_patches() {
		let dir = tempfile::tempdir().unwrap();
		let store = Store::open(&dir.path().join("reset.sqlite3")).unwrap();
		store
			.patch_settings(&serde_json::json!({"openaiModel":"other-writer","masterVolume":0.25}))
			.unwrap();
		let (saved, overlay) = store
			.patch_preferences(&serde_json::json!({"masterVolume":1.0}), false)
			.unwrap();
		assert_eq!(saved.openai_model, "other-writer");
		assert_eq!(saved.master_volume, 1.0);
		assert!(overlay.is_none());
		assert!(
			store
				.patch_preferences(&serde_json::json!({"masterVolume":10.0}), true)
				.is_err()
		);
		assert_eq!(store.settings().unwrap().master_volume, 1.0);
		assert!(
			store
				.get::<OverlaySettings>("overlay_settings")
				.unwrap()
				.is_none()
		);
	}
}
