//! Explicitly opted-in real-provider verification with no production executors.
//! The normal desktop binary does not compile this module.
use super::*;
use crate::providers::Providers;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
	pub model: String,
	pub catalog_tools: usize,
	pub requests: usize,
	pub executed_calls: usize,
	pub input_tokens: u64,
	pub output_tokens: u64,
	pub final_state: String,
	pub reply: String,
	pub duration_ms: u128,
}
struct ProbeHost {
	providers: Arc<Providers>,
	requests: AtomicUsize,
	input_tokens: AtomicU64,
	output_tokens: AtomicU64,
}
impl Host for ProbeHost {
	fn store(&self) -> &Store {
		&self.providers.store
	}
	fn definitions(&self) -> Vec<ToolDefinition> {
		tools::definitions()
	}
	async fn owner(&self, source: &ChatMessage) -> Result<bool> {
		ensure!(
			source.platform == "preview" && source.user_id == "owner",
			"Invalid probe actor"
		);
		Ok(true)
	}
	async fn request(
		&self,
		cp: &Checkpoint,
		defs: &[ToolDefinition],
		owner: bool,
		cancel: CancellationToken,
	) -> Result<Value> {
		ensure!(
			self.requests.fetch_add(1, Ordering::SeqCst) < 3,
			"Probe exceeded its three-request limit"
		);
		let response =
			model::request_with_providers(&self.providers, cp, defs, owner, cancel).await?;
		self.input_tokens.fetch_add(
			response["usage"]["input_tokens"].as_u64().unwrap_or(0),
			Ordering::SeqCst,
		);
		self.output_tokens.fetch_add(
			response["usage"]["output_tokens"].as_u64().unwrap_or(0),
			Ordering::SeqCst,
		);
		Ok(response)
	}
	async fn execute(
		&self,
		_: &mut Checkpoint,
		call: &ToolCall,
		args: &Value,
		_: CancellationToken,
	) -> Result<Value> {
		ensure!(
			call.name == "configureTurnDelivery",
			"Live probe forbids every non-delivery tool executor"
		);
		ensure!(
			args["speech"] == false
				&& args["publicProgress"] == false
				&& args["targets"] == json!(["source"])
				&& args["discordDmUserId"].is_null(),
			"Live probe forbids audio and external delivery"
		);
		Ok(
			json!({"status":"configured","delivery":{"speech":false,"publicProgress":false,"targets":["source"],"discordDmUserId":null}}),
		)
	}
	async fn prompt(&self, _: &Checkpoint, _: &PendingInput, _: CancellationToken) -> Result<()> {
		bail!("Live probe cannot prompt or dispatch a confirmation")
	}
	async fn finish(
		&self,
		_: &Checkpoint,
		reply: &FinalReply,
		_: CancellationToken,
	) -> Result<Value> {
		ensure!(
			!reply.text.is_empty() && reply.text.len() <= 500,
			"Expected one short greeting"
		);
		ensure!(
			reply
				.messages
				.as_ref()
				.is_none_or(|groups| groups.iter().all(|group| group.artifact_ids.is_empty())),
			"Probe cannot deliver artifacts"
		);
		self.store().set("agent_probe_reply", &reply.text)?;
		Ok(json!({"localOnly":true,"destination":"isolated probe SQLite"}))
	}
}

/// Runs the actual native drive/parser/schema/ledger with a non-network delivery
/// boundary. The caller must supply a new, isolated Store and OpenAI-only keys.
pub async fn run(providers: Arc<Providers>, model_id: &str) -> Result<ProbeReport> {
	ensure!(
		providers
			.store
			.db()?
			.query_row("SELECT COUNT(*) FROM agent_turns", [], |row| row
				.get::<_, i64>(0))?
			== 0,
		"Probe requires an isolated database without existing turns"
	);
	let host = ProbeHost {
		providers,
		requests: AtomicUsize::new(0),
		input_tokens: AtomicU64::new(0),
		output_tokens: AtomicU64::new(0),
	};
	let source=ChatMessage { platform:"preview".into(),user_id:"owner".into(),display_name:"Local integration probe".into(),channel_id:"probe".into(),message_id:uuid::Uuid::new_v4().to_string(),is_owner:true,text:"Greet me in one short sentence as Bumblebee. Configure delivery with speech=false, publicProgress=false, targets=[\"source\"], discordDmUserId=null. Use no other tools; do not send any messages or create files.".into() };
	let mut cp = Checkpoint {
		id: uuid::Uuid::new_v4().to_string(),
		source: source.clone(),
		model: model_id.into(),
		items: vec![json!({"role":"user","content":source.text})],
		rounds: 0,
		executed: 0,
		calls: vec![],
		cursor: 0,
		delivery: None,
		artifacts: vec![],
		pending: None,
		answer: None,
		approved_call: None,
		voice_channel_id: None,
	};
	host.store().create_turn(
		&cp.id,
		&durable::actor(&source),
		&serde_json::to_value(&cp)?,
	)?;
	host
		.store()
		.append_history(&durable::conversation_scope(&source), "user", &source.text)?;
	let started = std::time::Instant::now();
	let cancel = CancellationToken::new();
	let result = tokio::time::timeout(
		std::time::Duration::from_secs(90),
		run_guarded(&host, &mut cp, cancel.clone()),
	)
	.await;
	cancel.cancel();
	result.context("Live probe exceeded 90 seconds")??;
	let turn = host.store().turn(&cp.id)?;
	ensure!(
		turn.state == "completed" && cp.executed == 1 && cp.delivery.is_some(),
		"Probe did not finish one delivery configuration and a final reply"
	);
	let reply: String = host
		.store()
		.get("agent_probe_reply")?
		.context("Probe final reply was not saved")?;
	let history = host
		.store()
		.history(&durable::conversation_scope(&source))?;
	ensure!(
		history
			.iter()
			.any(|message| message["role"] == "assistant" && message["content"] == reply),
		"Final assistant history was not persisted"
	);
	Ok(ProbeReport {
		model: model_id.into(),
		catalog_tools: host.definitions().len(),
		requests: host.requests.load(Ordering::SeqCst),
		executed_calls: cp.executed,
		input_tokens: host.input_tokens.load(Ordering::SeqCst),
		output_tokens: host.output_tokens.load(Ordering::SeqCst),
		final_state: turn.state,
		reply,
		duration_ms: started.elapsed().as_millis(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::providers::SecretStore;
	struct NoSecrets;
	impl SecretStore for NoSecrets {
		fn get(&self, _: &str) -> Result<Option<String>> {
			bail!("This guard test must not read credentials")
		}
		fn set(&self, _: &str, _: &str) -> Result<()> {
			bail!("No credential writes")
		}
		fn delete(&self, _: &str) -> Result<()> {
			bail!("No credential deletion")
		}
	}
	#[tokio::test]
	async fn probe_boundary_cannot_dispatch_tools_audio_or_external_delivery() {
		let data = tempfile::tempdir().unwrap();
		let store = Arc::new(Store::open(&data.path().join("state.sqlite")).unwrap());
		let host = ProbeHost {
			providers: Providers::new(store, Arc::new(NoSecrets)).unwrap(),
			requests: AtomicUsize::new(0),
			input_tokens: AtomicU64::new(0),
			output_tokens: AtomicU64::new(0),
		};
		let mut cp:Checkpoint=serde_json::from_value(json!({"id":"probe","source":{"platform":"preview","user_id":"owner","display_name":"Probe","channel_id":"probe","message_id":"probe","text":"Hello","is_owner":true},"model":"unused","items":[],"rounds":0,"executed":0,"calls":[],"cursor":0,"delivery":null,"artifacts":[],"pending":null,"answer":null,"approved_call":null})).unwrap();
		let call = ToolCall {
			id: "configure".into(),
			name: "configureTurnDelivery".into(),
			arguments: String::new(),
		};
		let valid =
			json!({"speech":false,"publicProgress":false,"targets":["source"],"discordDmUserId":null});
		assert!(
			host
				.execute(&mut cp, &call, &valid, CancellationToken::new())
				.await
				.is_ok()
		);
		for (key, value) in [
			("speech", json!(true)),
			("publicProgress", json!(true)),
			("targets", json!(["discord_dm"])),
			("discordDmUserId", json!("123")),
		] {
			let mut args = valid.clone();
			args[key] = value;
			assert!(
				host
					.execute(&mut cp, &call, &args, CancellationToken::new())
					.await
					.is_err()
			);
		}
		let external = ToolCall {
			name: "deliverMessage".into(),
			..call
		};
		assert!(
			host
				.execute(&mut cp, &external, &valid, CancellationToken::new())
				.await
				.is_err()
		);
		assert_eq!(host.requests.load(Ordering::SeqCst), 0);
	}
}
