//! Preserve observed message acknowledgements when a later part fails.
use anyhow::Result;
use serde_json::{Value, json};
use std::future::Future;

/// A deliberately narrow projection for the trusted desktop recovery view.
/// Provider bodies, request text, tool arguments and credentials never cross it.
#[derive(Clone, Debug, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReceipt {
	pub status: String,
	pub destination: Option<String>,
	pub message_ids: Vec<String>,
	pub completed_parts: Option<u64>,
	pub total_parts: Option<u64>,
	pub unacknowledged_part_may_have_sent: Option<bool>,
}

const MAX_RECOVERY_RECEIPTS: usize = 128;
const MAX_RECEIPT_MESSAGE_IDS: usize = 128;

impl crate::storage::Store {
	/// Observe saved acknowledgements without retrying or returning raw context.
	pub fn interruption_receipts(&self, id: &str) -> Result<Vec<RecoveryReceipt>> {
		let db = self.db()?;
		let final_receipt: Option<String> = db.query_row(
			"SELECT CASE WHEN json_valid(checkpoint) THEN json_extract(checkpoint,'$.partialDelivery') END FROM agent_turns WHERE id=?",
			[id], |row| row.get(0),
		)?;
		let mut query = db.prepare(
			"SELECT json_extract(result,'$.receipt') FROM tool_calls WHERE turn_id=? AND json_valid(result) AND json_type(result,'$.receipt')='object' ORDER BY rowid LIMIT 96",
		)?;
		let tool_receipts = query
			.query_map([id], |row| row.get::<_, String>(0))?
			.collect::<rusqlite::Result<Vec<_>>>()?;
		drop(query);
		drop(db);
		let mut receipts = Vec::new();
		for saved in final_receipt.into_iter().chain(tool_receipts) {
			if let Ok(value) = serde_json::from_str::<Value>(&saved) {
				project_receipts(&value, None, "unknown", 0, &mut receipts);
			}
		}
		Ok(receipts)
	}
}

fn safe_identifier(value: &Value) -> Option<String> {
	let value = value.as_str()?;
	(!value.is_empty()
		&& value.len() <= 256
		&& value
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.')))
	.then(|| value.to_owned())
}

fn project_receipts(
	value: &Value,
	target: Option<&str>,
	fallback_status: &str,
	depth: usize,
	out: &mut Vec<RecoveryReceipt>,
) {
	if depth > 6 || out.len() >= MAX_RECOVERY_RECEIPTS || !value.is_object() {
		return;
	}
	if let Some(completed) = value["completedDeliveries"].as_array() {
		for delivery in completed.iter().take(MAX_RECOVERY_RECEIPTS) {
			project_receipts(
				&delivery["receipt"],
				delivery["target"].as_str(),
				"delivered",
				depth + 1,
				out,
			);
		}
		if value["failedDelivery"].is_object() {
			project_receipts(&value["failedDelivery"], target, "unknown", depth + 1, out);
		}
		return;
	}
	let status = value["status"]
		.as_str()
		.filter(|status| matches!(*status, "delivered" | "partial" | "failed" | "unknown"))
		.unwrap_or(fallback_status)
		.to_owned();
	let destination = safe_identifier(&value["destination"])
		.or_else(|| safe_identifier(&value["channelId"]).map(|id| format!("discord:{id}")))
		.or_else(|| {
			target
				.filter(|target| matches!(*target, "source" | "discord_dm"))
				.map(str::to_owned)
		});
	let message_ids = value["messageIds"]
		.as_array()
		.map(|ids| {
			ids.iter()
				.take(MAX_RECEIPT_MESSAGE_IDS)
				.filter_map(safe_identifier)
				.collect()
		})
		.unwrap_or_default();
	out.push(RecoveryReceipt {
		status,
		destination,
		message_ids,
		completed_parts: value["completedParts"].as_u64(),
		total_parts: value["totalParts"].as_u64(),
		unacknowledged_part_may_have_sent: value["unacknowledgedPartMayHaveSent"].as_bool(),
	});
}

#[derive(Debug)]
pub(crate) struct DeliveryFailure {
	pub receipt: Value,
}
impl std::fmt::Display for DeliveryFailure {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Message delivery was not completed; inspect its saved receipts before retrying")
	}
}
impl std::error::Error for DeliveryFailure {}

/// Only a definite provider rejection may assert that this attempted part was
/// not sent. Transport errors and unreadable/missing acknowledgements stay unknown.
#[derive(Debug)]
pub(crate) struct RejectedDelivery;
impl std::fmt::Display for RejectedDelivery {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("Provider rejected this message before delivery")
	}
}
impl std::error::Error for RejectedDelivery {}

pub(crate) fn delivery_failure_receipt(error: &anyhow::Error) -> Option<Value> {
	error
		.downcast_ref::<DeliveryFailure>()
		.map(|error| error.receipt.clone())
}

pub(crate) async fn send_parts<F, Fut>(
	destination: &str,
	parts: &[String],
	mut send: F,
) -> Result<Vec<String>>
where
	F: FnMut(usize, String) -> Fut,
	Fut: Future<Output = Result<String>>,
{
	let mut ids = Vec::new();
	for (index, part) in parts.iter().enumerate() {
		match send(index, part.clone()).await {
			Ok(id) if !id.is_empty() => ids.push(id),
			outcome => {
				let rejected = outcome
					.as_ref()
					.err()
					.is_some_and(|error| error.downcast_ref::<RejectedDelivery>().is_some());
				return Err(DeliveryFailure{receipt:json!({
					"destination":destination,"messageIds":ids,"completedParts":ids.len(),"totalParts":parts.len(),
					"attemptedParts":index+1,"failedPart":index,"unacknowledgedPartMayHaveSent":!rejected,
					"status":if !ids.is_empty(){"partial"}else if rejected{"failed"}else{"unknown"}
				})}.into());
			}
		}
	}
	Ok(ids)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicUsize, Ordering};

	#[test]
	fn recovery_projection_flattens_saved_receipts_without_private_context() {
		let store = crate::storage::Store::open(std::path::Path::new(":memory:")).unwrap();
		store.create_turn("turn", "preview:owner", &json!({"source":{"text":"private request"},"partialDelivery":{
			"status":"partial", "error":"private provider error", "completedDeliveries":[
				{"target":"discord_dm","receipt":{"messageIds":["known-dm"],"channelId":"123","content":"private message"}}
			], "failedDelivery":{"status":"partial","destination":"discord:456","messageIds":["known-public"],"completedParts":1,"totalParts":3,"unacknowledgedPartMayHaveSent":true,"token":"private credential"}
		}})).unwrap();
		store
			.begin_call(
				"turn",
				"call",
				"deliverMessage",
				"{\"content\":\"private tool args\"}",
				crate::storage::CallEffect::MayMutate,
			)
			.unwrap();
		store.record_unknown_call("turn", "call", &json!({"status":"unknown","error":"private raw response","receipt":{"status":"unknown","destination":"twitch:789","messageIds":[],"completedParts":0,"totalParts":2,"unacknowledgedPartMayHaveSent":true}}).to_string()).unwrap();
		let receipts = store.interruption_receipts("turn").unwrap();
		assert_eq!(receipts.len(), 3);
		assert_eq!(receipts[0].status, "delivered");
		assert_eq!(receipts[0].destination.as_deref(), Some("discord:123"));
		assert_eq!(receipts[0].message_ids, vec!["known-dm"]);
		assert_eq!(receipts[0].completed_parts, None);
		assert_eq!(receipts[1].completed_parts, Some(1));
		assert_eq!(receipts[1].total_parts, Some(3));
		assert_eq!(receipts[2].destination.as_deref(), Some("twitch:789"));
		let projected = serde_json::to_string(&receipts).unwrap();
		assert!(!projected.contains("private"));
		assert!(!projected.contains("token"));
		assert!(!projected.contains("error"));
	}

	#[test]
	fn recovery_projection_bounds_saved_lists_and_rejects_non_identifiers() {
		let entry = json!({"target":"source","receipt":{"status":"unexpected private status","destination":"https://example.invalid?token=private","messageIds":["okay-id","contains spaces",{"private":"object"}],"completedParts":-1}});
		let value = json!({"completedDeliveries":vec![entry;MAX_RECOVERY_RECEIPTS+20],"failedDelivery":{"status":"unknown"}});
		let mut rows = Vec::new();
		project_receipts(&value, None, "unknown", 0, &mut rows);
		assert_eq!(rows.len(), MAX_RECOVERY_RECEIPTS);
		assert_eq!(rows[0].status, "delivered");
		assert_eq!(rows[0].destination.as_deref(), Some("source"));
		assert_eq!(rows[0].message_ids, vec!["okay-id"]);
		assert_eq!(rows[0].completed_parts, None);
	}
	#[tokio::test]
	async fn later_rejection_keeps_exact_acknowledgements_and_never_sends_remaining_parts() {
		let calls = AtomicUsize::new(0);
		let parts = vec!["first".into(), "second".into(), "third".into()];
		let error = send_parts("discord:123", &parts, |index, _| {
			calls.fetch_add(1, Ordering::SeqCst);
			async move {
				if index == 0 {
					Ok("observed-id".into())
				} else {
					tokio::task::yield_now().await;
					Err(RejectedDelivery.into())
				}
			}
		})
		.await
		.unwrap_err();
		let receipt = delivery_failure_receipt(&error).unwrap();
		assert_eq!(calls.load(Ordering::SeqCst), 2);
		assert_eq!(receipt["messageIds"], json!(["observed-id"]));
		assert_eq!(receipt["completedParts"], 1);
		assert_eq!(receipt["totalParts"], 3);
		assert_eq!(receipt["status"], "partial");
		assert_eq!(receipt["unacknowledgedPartMayHaveSent"], false);
	}
	#[tokio::test]
	async fn first_rejection_and_unreadable_ack_are_distinct_without_retry() {
		for rejected in [false, true] {
			let calls = AtomicUsize::new(0);
			let error = send_parts(
				"discord:123",
				&["message".into(), "later".into()],
				|_, _| {
					calls.fetch_add(1, Ordering::SeqCst);
					async move {
						if rejected {
							Err(RejectedDelivery.into())
						} else {
							anyhow::bail!("Acknowledged body was unreadable")
						}
					}
				},
			)
			.await
			.unwrap_err();
			let receipt = delivery_failure_receipt(&error).unwrap();
			assert_eq!(calls.load(Ordering::SeqCst), 1);
			assert_eq!(receipt["messageIds"], json!([]));
			assert_eq!(receipt["completedParts"], 0);
			assert_eq!(receipt["totalParts"], 2);
			assert_eq!(
				receipt["status"],
				if rejected { "failed" } else { "unknown" }
			);
			assert_eq!(receipt["unacknowledgedPartMayHaveSent"], !rejected);
		}
	}
}
