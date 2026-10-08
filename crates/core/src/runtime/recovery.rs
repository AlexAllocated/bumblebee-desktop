use super::*;
use crate::agent_storage::TurnRecord;

enum Readiness {
	Ready,
	Waiting,
	Unavailable(String),
}
impl Engine {
	/// Explicit retry after repairing setup. This only queues positively marked
	/// crash recovery; all requester, source, and permission checks run again.
	pub async fn resume_interrupted(&self, id: &str) -> Result<()> {
		ensure!(
			self.is_active(),
			"Start a session before resuming interrupted work"
		);
		let record = self.store.turn(id)?;
		ensure!(
			record.resumable,
			"This request cannot resume: only recent crash-interrupted work is eligible; unknown delivery and cancelled work must not replay"
		);
		let source = crate::agent::recoverable_source(&record)?;
		let epoch = self
			.session
			.lock()
			.await
			.as_ref()
			.context("Start a session before resuming")?
			.agent_scopes
			.epoch();
		match self.recovery_readiness(&source).await? {
			Readiness::Ready => {}
			Readiness::Waiting => anyhow::bail!(
				"The original connection is not ready. Reconnect it, then choose Resume again"
			),
			Readiness::Unavailable(reason) => anyhow::bail!(reason),
		}
		ensure!(
			self.enqueue_recovery(&record, &epoch).await?,
			"The conversation queue is full; try Resume again shortly"
		);
		self.mark_recovery_attempt(id);
		Ok(())
	}

	async fn recovery_readiness(&self, source: &ChatMessage) -> Result<Readiness> {
		let settings = self.store.settings()?;
		if !settings.ai_enabled {
			return Ok(Readiness::Unavailable(
				"Enable the agent before continuing saved requests".into(),
			));
		}
		match source.platform.as_str() {
			"preview" => Ok(Readiness::Ready),
			"twitch" | "youtube" => {
				let key = if source.platform == "twitch" {
					"twitch_tokens"
				} else {
					"google_tokens"
				};
				if self.providers.secrets.get(key)?.is_none() {
					return Ok(Readiness::Unavailable(format!(
						"Reconnect {} to continue this saved request",
						source.platform
					)));
				}
				match self.providers.stream_chat_destination(&source.platform) {
					Some(channel) if channel == source.channel_id => Ok(Readiness::Ready),
					Some(_) => Ok(Readiness::Unavailable(
						"The original chat channel changed; the saved request remains interrupted".into(),
					)),
					None => Ok(Readiness::Waiting),
				}
			}
			"discord" | "discord_voice" => {
				if self.providers.secrets.get("discord_bot")?.is_none()
					|| settings.discord_guild_id.is_empty()
				{
					return Ok(Readiness::Unavailable(
						"Configure Discord before continuing this saved request".into(),
					));
				}
				let Some(audio) = self.audio().await else {
					return Ok(Readiness::Waiting);
				};
				if source.platform == "discord_voice" && !audio.has_voice_presence() {
					return Ok(Readiness::Waiting);
				}
				Ok(Readiness::Ready)
			}
			_ => Ok(Readiness::Unavailable(
				"The saved request's integration is unavailable".into(),
			)),
		}
	}
	fn mark_recovery_attempt(&self, id: &str) -> bool {
		self
			.recovery_attempts
			.lock()
			.map(|mut attempts| attempts.insert(id.into()))
			.unwrap_or(false)
	}
	async fn enqueue_recovery(
		&self,
		record: &TurnRecord,
		cancel: &CancellationToken,
	) -> Result<bool> {
		let mut session = self.session.lock().await;
		let Some(session) = session
			.as_mut()
			.filter(|session| !session.cancel.is_cancelled())
		else {
			return Ok(false);
		};
		if cancel.is_cancelled() {
			return Ok(false);
		}
		let sender = session.agent_tx.clone();
		let Ok(permit) = sender.try_reserve() else {
			return Ok(false);
		};
		if !self.store.stage_recovery(&record.id, &record.actor)? {
			return Ok(true);
		}
		let scope = session.agent_scopes.lease(&record.actor);
		permit.send(AgentJob {
			input: AgentInput::Recovery {
				turn_id: record.id.clone(),
			},
			scope,
		});
		Ok(true)
	}
	pub(super) async fn recovery_loop(self: Arc<Self>, cancel: CancellationToken) {
		let mut candidates = match self.store.recovery_candidates() {
			Ok(records) => records,
			Err(error) => {
				self.emit(OverlayEvent::Status {
					message: error.to_string(),
				});
				return;
			}
		};
		if let Ok(attempts) = self.recovery_attempts.lock() {
			candidates.retain(|record| !attempts.contains(&record.id));
		}
		let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
		let mut tick = tokio::time::interval(Duration::from_millis(500));
		while !candidates.is_empty() {
			tokio::select! {biased;_=cancel.cancelled()=>return,_=tick.tick()=>{}}
			let mut waiting = Vec::new();
			for record in candidates {
				if cancel.is_cancelled() {
					return;
				}
				let source = match crate::agent::recoverable_source(&record) {
					Ok(source) => source,
					Err(error) => {
						self.mark_recovery_attempt(&record.id);
						self.emit(OverlayEvent::Status {
							message: format!("Saved request could not resume: {error}"),
						});
						continue;
					}
				};
				let ready = self.recovery_readiness(&source).await;
				match ready {
					Ok(Readiness::Ready) => match self.enqueue_recovery(&record, &cancel).await {
						Ok(true) => {
							self.mark_recovery_attempt(&record.id);
						}
						Ok(false) if tokio::time::Instant::now() < deadline => waiting.push(record),
						Ok(false) => {
							self.mark_recovery_attempt(&record.id);
							self.emit(OverlayEvent::Status{message:"Saved request remains interrupted because the conversation queue is busy".into()});
						}
						Err(error) => {
							self.mark_recovery_attempt(&record.id);
							self.emit(OverlayEvent::Status {
								message: error.to_string(),
							});
						}
					},
					Ok(Readiness::Waiting) if tokio::time::Instant::now() < deadline => {
						waiting.push(record)
					}
					Ok(Readiness::Waiting) => {
						self.mark_recovery_attempt(&record.id);
						self.emit(OverlayEvent::Status {
							message: format!(
								"Saved {} request remains interrupted because its original connection is not ready",
								source.platform
							),
						});
					}
					Ok(Readiness::Unavailable(reason)) => {
						self.mark_recovery_attempt(&record.id);
						self.emit(OverlayEvent::Status { message: reason });
					}
					Err(error) => {
						self.mark_recovery_attempt(&record.id);
						self.emit(OverlayEvent::Status {
							message: error.to_string(),
						});
					}
				}
			}
			candidates = waiting;
		}
	}
}
