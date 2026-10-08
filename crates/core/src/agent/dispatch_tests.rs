use super::*;
struct NoSecrets;
impl crate::providers::SecretStore for NoSecrets {
	fn get(&self, name: &str) -> Result<Option<String>> {
		assert_ne!(
			name, "openai",
			"Offline dispatch must not request the model"
		);
		Ok(None)
	}
	fn set(&self, _: &str, _: &str) -> Result<()> {
		anyhow::bail!("No fixture credentials")
	}
	fn delete(&self, _: &str) -> Result<()> {
		anyhow::bail!("No fixture credentials")
	}
}
fn fixture() -> (tempfile::TempDir, RuntimeHost, Checkpoint, ToolCall, Value) {
	let dir = tempfile::tempdir().unwrap();
	let store = Arc::new(Store::open(&dir.path().join("test.db")).unwrap());
	let mut settings = store.settings().unwrap();
	settings.ai_enabled = true;
	store.set("installation", &settings).unwrap();
	let providers = crate::providers::Providers::new(store, Arc::new(NoSecrets)).unwrap();
	let engine = Engine::new(
		providers,
		crate::runtime::EnginePaths {
			data_dir: dir.path().into(),
			native_dir: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
				.join("../../src-tauri/resources"),
		},
		tokio::sync::broadcast::channel(32).0,
	)
	.unwrap();
	let cp:Checkpoint=serde_json::from_value(json!({"id":"dispatch","source":{"platform":"preview","user_id":"owner","display_name":"Owner","channel_id":"preview","message_id":"message","text":"Configure delivery","is_owner":true},"model":"unused-fixture","items":[],"rounds":1,"executed":1,"calls":[],"cursor":0,"delivery":null,"artifacts":[],"pending":null,"answer":null,"approved_call":null,"requester_was_owner":true,"recovery_eligible":true})).unwrap();
	let args =
		json!({"speech":true,"publicProgress":true,"targets":["source"],"discordDmUserId":null});
	let call = ToolCall {
		id: "configure".into(),
		name: "configureTurnDelivery".into(),
		arguments: args.to_string(),
	};
	(dir, RuntimeHost { engine }, cp, call, args)
}
#[test]
fn real_runtime_configure_and_settings_dispatch_fit_normal_worker_stack() {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(1)
		.thread_stack_size(2 * 1024 * 1024)
		.enable_all()
		.build()
		.unwrap();
	runtime.block_on(async {
		tokio::spawn(async {
			let (_dir, host, mut cp, call, _) = fixture();
			let mut disable = json!({});
			for key in tools::definitions()
				.iter()
				.find(|d| d.name == "setAiSettings")
				.unwrap()
				.parameters["properties"]
				.as_object()
				.unwrap()
				.keys()
			{
				disable[key] = Value::Null;
			}
			disable["enabled"] = json!(false);
			cp.calls = vec![
				call,
				ToolCall {
					id: "disable".into(),
					name: "setAiSettings".into(),
					arguments: disable.to_string(),
				},
			];
			host
				.store()
				.create_turn(&cp.id, "preview:owner", &serde_json::to_value(&cp).unwrap())
				.unwrap();
			let error = run_guarded(&host, &mut cp, CancellationToken::new())
				.await
				.unwrap_err();
			assert!(error.to_string().contains("disabled"));
			assert!(!host.store().settings().unwrap().ai_enabled);
			assert_eq!(
				host
					.store()
					.saved_call(
						&cp.id,
						"configure",
						"configureTurnDelivery",
						&cp.calls[0].arguments
					)
					.unwrap()
					.unwrap()
					.0,
				"completed"
			);
		})
		.await
		.unwrap()
	});
}

#[test]
fn queued_recovery_dispatch_fits_normal_worker_stack() {
	let runtime = tokio::runtime::Builder::new_multi_thread()
		.worker_threads(1)
		.thread_stack_size(2 * 1024 * 1024)
		.enable_all()
		.build()
		.unwrap();
	runtime.block_on(async {
		tokio::spawn(async {
			let (_dir, host, mut cp, call, _) = fixture();
			let mut disable = json!({});
			for key in tools::definitions()
				.iter()
				.find(|d| d.name == "setAiSettings")
				.unwrap()
				.parameters["properties"]
				.as_object()
				.unwrap()
				.keys()
			{
				disable[key] = Value::Null;
			}
			disable["enabled"] = json!(false);
			cp.calls = vec![
				call,
				ToolCall {
					id: "disable".into(),
					name: "setAiSettings".into(),
					arguments: disable.to_string(),
				},
			];
			host
				.store()
				.create_turn(&cp.id, "preview:owner", &serde_json::to_value(&cp).unwrap())
				.unwrap();
			host.store().recover_interrupted().unwrap();
			host.engine.start().await.unwrap();
			tokio::time::timeout(std::time::Duration::from_secs(3), async {
				while matches!(
					host.store().turn(&cp.id).unwrap().state.as_str(),
					"interrupted" | "queued_recovery" | "running"
				) {
					tokio::time::sleep(std::time::Duration::from_millis(10)).await;
				}
			})
			.await
			.unwrap();
			assert_eq!(host.store().turn(&cp.id).unwrap().state, "failed");
			for call in &cp.calls {
				assert_eq!(
					host
						.store()
						.saved_call(&cp.id, &call.id, &call.name, &call.arguments)
						.unwrap()
						.unwrap()
						.0,
					"completed"
				);
			}
			assert!(!host.store().settings().unwrap().ai_enabled);
			host.engine.stop().await.unwrap();
		})
		.await
		.unwrap()
	});
}
