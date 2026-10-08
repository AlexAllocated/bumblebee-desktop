//! Cancellation is stamped when work is submitted, not when a worker eventually dequeues it.
use crate::model::ChatMessage;
use std::{
	collections::HashMap,
	future::Future,
	sync::{Arc, Weak},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub(super) struct ActorScope {
	pub cancel: CancellationToken,
}

pub(super) struct WorkScopes {
	epoch: CancellationToken,
	actors: HashMap<String, Weak<ActorScope>>,
}

impl WorkScopes {
	pub fn new(parent: &CancellationToken) -> Self {
		Self {
			epoch: parent.child_token(),
			actors: HashMap::new(),
		}
	}
	pub fn epoch(&self) -> CancellationToken {
		self.epoch.clone()
	}
	pub fn lease(&mut self, actor: &str) -> Arc<ActorScope> {
		self.actors.retain(|_, scope| scope.strong_count() > 0);
		if let Some(scope) = self.actors.get(actor).and_then(Weak::upgrade) {
			return scope;
		}
		let scope = Arc::new(ActorScope {
			cancel: self.epoch.child_token(),
		});
		self.actors.insert(actor.into(), Arc::downgrade(&scope));
		scope
	}
	pub fn cancel_all(&mut self, parent: &CancellationToken) -> CancellationToken {
		self.epoch.cancel();
		self.epoch = parent.child_token();
		self.actors.clear();
		self.epoch()
	}
	pub fn cancel_actor(&mut self, actor: &str) -> bool {
		if let Some(scope) = self.actors.remove(actor).and_then(|scope| scope.upgrade()) {
			scope.cancel.cancel();
			true
		} else {
			false
		}
	}
}

pub(super) enum AgentInput {
	Message(ChatMessage),
	Recovery { turn_id: String },
}
pub(super) struct AgentJob {
	pub input: AgentInput,
	pub scope: Arc<ActorScope>,
}

pub(super) async fn run_agent_queue<F, Fut>(
	mut receiver: mpsc::Receiver<AgentJob>,
	session: CancellationToken,
	mut run: F,
) where
	F: FnMut(AgentInput, CancellationToken) -> Fut,
	Fut: Future<Output = ()>,
{
	loop {
		let job = tokio::select! {biased;_=session.cancelled()=>break, job=receiver.recv()=>job};
		let Some(job) = job else { break };
		// Keep the lease alive during execution so actor cancellation covers active and queued work.
		let scope = job.scope;
		if scope.cancel.is_cancelled() && matches!(job.input, AgentInput::Message(_)) {
			continue;
		}
		run(job.input, scope.cancel.child_token()).await;
	}
	// Recovery jobs are durable: explicit session cancellation must not leave
	// their queued records eligible for the next process to resurrect.
	while let Ok(job) = receiver.try_recv() {
		if matches!(job.input, AgentInput::Recovery { .. }) {
			let cancelled = CancellationToken::new();
			cancelled.cancel();
			run(job.input, cancelled).await;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Mutex;
	use tokio::sync::Notify;

	fn message(id: &str, actor: &str) -> ChatMessage {
		ChatMessage {
			platform: "preview".into(),
			user_id: actor.into(),
			display_name: actor.into(),
			message_id: id.into(),
			channel_id: "stream".into(),
			text: "!bee test".into(),
			is_owner: false,
			access: Default::default(),
		}
	}
	async fn interrupted_queue(actor_only: bool) {
		let session = CancellationToken::new();
		let mut scopes = WorkScopes::new(&session);
		let (tx, rx) = mpsc::channel(16);
		for (id, actor) in [
			("active", "alice"),
			("queued-alice", "alice"),
			("queued-bob", "bob"),
		] {
			tx.send(AgentJob {
				input: AgentInput::Message(message(id, actor)),
				scope: scopes.lease(actor),
			})
			.await
			.unwrap();
		}
		let started = Arc::new(Notify::new());
		let finished = Arc::new(Notify::new());
		let executions = Arc::new(Mutex::new(Vec::new()));
		let worker = tokio::spawn(run_agent_queue(rx, session.clone(), {
			let executions = executions.clone();
			let started = started.clone();
			let finished = finished.clone();
			move |input, cancel| {
				let AgentInput::Message(message) = input else {
					panic!("Unexpected recovery")
				};
				let executions = executions.clone();
				let started = started.clone();
				let finished = finished.clone();
				async move {
					executions.lock().unwrap().push(message.message_id.clone());
					if message.message_id == "active" {
						started.notify_one();
						cancel.cancelled().await;
					}
					if message.message_id == "fresh" {
						finished.notify_one();
					}
				}
			}
		}));
		started.notified().await;
		if actor_only {
			assert!(scopes.cancel_actor("alice"));
		} else {
			scopes.cancel_all(&session);
		}
		tx.send(AgentJob {
			input: AgentInput::Message(message("fresh", "alice")),
			scope: scopes.lease("alice"),
		})
		.await
		.unwrap();
		tokio::time::timeout(std::time::Duration::from_secs(2), finished.notified())
			.await
			.unwrap();
		session.cancel();
		worker.await.unwrap();
		let expected = if actor_only {
			vec!["active", "queued-bob", "fresh"]
		} else {
			vec!["active", "fresh"]
		};
		assert_eq!(*executions.lock().unwrap(), expected);
	}
	#[tokio::test]
	async fn global_cancel_discards_queued_turns_but_accepts_fresh_requests() {
		interrupted_queue(false).await;
	}
	#[tokio::test]
	async fn actor_cancel_stops_their_active_and_queued_turns_without_canceling_other_viewers() {
		interrupted_queue(true).await;
	}
	#[tokio::test]
	async fn stopped_session_cannot_dequeue_work_even_when_both_branches_are_ready() {
		let session = CancellationToken::new();
		let mut scopes = WorkScopes::new(&session);
		let (tx, rx) = mpsc::channel(1);
		tx.send(AgentJob {
			input: AgentInput::Message(message("old", "alice")),
			scope: scopes.lease("alice"),
		})
		.await
		.unwrap();
		session.cancel();
		run_agent_queue(rx, session, |_, _| async {
			panic!("stopped session dispatched a turn")
		})
		.await;
	}
	#[test]
	fn completed_actor_leases_do_not_accumulate_for_every_past_chatter() {
		let mut scopes = WorkScopes::new(&CancellationToken::new());
		for i in 0..1000 {
			drop(scopes.lease(&i.to_string()));
		}
		assert!(scopes.actors.len() <= 1);
	}
}
