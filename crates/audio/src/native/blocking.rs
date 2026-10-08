use std::{
	sync::atomic::{AtomicU64, Ordering},
	time::Instant,
};

use serde_json::{Value, json};

static ACTIVE: AtomicU64 = AtomicU64::new(0);
static COMPLETED: AtomicU64 = AtomicU64::new(0);
static SLOW: AtomicU64 = AtomicU64::new(0);
static MAX_DURATION_MS: AtomicU64 = AtomicU64::new(0);
const SLOW_CALL_MS: u64 = 250;

/// Native SDK calls are synchronous, even when their API mentions async/poll.
/// Hand the Tokio worker's other tasks off before entering FFI. This keeps the
/// caller's ordering/ownership and creates no detached per-frame task queue.
/// Production uses a multi-thread runtime; synchronous native tests need none.
pub fn run<T>(operation: &'static str, work: impl FnOnce() -> T) -> T {
	tokio::task::block_in_place(|| {
		ACTIVE.fetch_add(1, Ordering::Relaxed);
		let _call = Call {
			operation,
			started: Instant::now(),
		};
		work()
	})
}

struct Call {
	operation: &'static str,
	started: Instant,
}

impl Drop for Call {
	fn drop(&mut self) {
		let elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
		ACTIVE.fetch_sub(1, Ordering::Relaxed);
		COMPLETED.fetch_add(1, Ordering::Relaxed);
		MAX_DURATION_MS.fetch_max(elapsed_ms, Ordering::Relaxed);
		if elapsed_ms >= SLOW_CALL_MS {
			SLOW.fetch_add(1, Ordering::Relaxed);
			crate::logging::log_event(
				tracing::Level::WARN,
				"songbird.native.slow_call",
				"native audio operation was slow",
				json!({ "data": { "operation": self.operation, "elapsedMs": elapsed_ms } }),
			);
		}
	}
}

pub fn snapshot() -> Value {
	json!({
		 "activeCalls": ACTIVE.load(Ordering::Relaxed),
		 "completedCalls": COMPLETED.load(Ordering::Relaxed),
		 "slowCalls": SLOW.load(Ordering::Relaxed),
		 "maxDurationMs": MAX_DURATION_MS.load(Ordering::Relaxed),
		 "slowCallThresholdMs": SLOW_CALL_MS,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::{Router, routing::get};
	use std::{
		io::{Read, Write},
		net::TcpStream,
		sync::mpsc,
		time::Duration,
	};

	fn health_during_blocking_call(isolate: bool) -> bool {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(1)
			.enable_all()
			.build()
			.unwrap();
		let (address, server) = runtime.block_on(async {
			let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
			let address = listener.local_addr().unwrap();
			let server = tokio::spawn(async move {
				axum::serve(
					listener,
					Router::new().route(
						"/live",
						get(|| async { axum::Json(serde_json::json!({"ok":true})) }),
					),
				)
				.await
				.unwrap();
			});
			(address, server)
		});
		let (entered_tx, entered_rx) = mpsc::channel();
		let (release_tx, release_rx) = mpsc::channel();
		let worker = runtime.spawn(async move {
			let work = || {
				entered_tx.send(()).unwrap();
				// An upper bound keeps even a failed test from hanging shutdown.
				release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
			};
			if isolate {
				run("test.blocked_native_call", work);
			} else {
				work();
			}
		});
		entered_rx.recv_timeout(Duration::from_secs(3)).unwrap();
		let response = (|| -> std::io::Result<String> {
			let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))?;
			stream.set_read_timeout(Some(Duration::from_millis(400)))?;
			stream.write_all(b"GET /live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
			let mut response = String::new();
			stream.read_to_string(&mut response)?;
			Ok(response)
		})();
		if isolate {
			assert!(snapshot()["activeCalls"].as_u64().unwrap() >= 1);
		}
		let _ = release_tx.send(());
		runtime.block_on(worker).unwrap();
		server.abort();
		runtime.shutdown_timeout(Duration::from_secs(1));
		response.is_ok_and(|body| body.starts_with("HTTP/1.1 200") && body.contains("\"ok\":true"))
	}

	#[test]
	fn unmarked_native_block_reproduces_liveness_starvation() {
		assert!(!health_during_blocking_call(false));
	}

	#[test]
	fn isolated_native_block_keeps_actual_liveness_route_responsive_on_one_worker() {
		assert!(health_during_blocking_call(true));
	}

	#[test]
	fn synchronous_native_callers_need_no_runtime() {
		assert_eq!(run("test.sync", || 42), 42);
	}

	#[test]
	#[ignore = "requires prepared native resources; run explicitly after prepare-native-audio.py"]
	fn real_keyword_lifecycle_runs_on_one_worker() {
		crate::NativeResources::from_bundle(
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources"),
		)
		.initialize()
		.unwrap();
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(1)
			.enable_all()
			.build()
			.unwrap();
		let (address, server) = runtime.block_on(async {
			let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
			let address = listener.local_addr().unwrap();
			let server = tokio::spawn(async move {
				axum::serve(
					listener,
					Router::new().route(
						"/live",
						get(|| async { axum::Json(serde_json::json!({"ok":true})) }),
					),
				)
				.await
				.unwrap();
			});
			(address, server)
		});
		let worker = runtime.spawn(async {
			for _ in 0..2 {
				let mut pipeline = crate::keyword::KeywordPipeline::new(
					"offline-regression",
					crate::keyword::KeywordModelSettings::default(),
				)
				.unwrap();
				for _ in 0..100 {
					pipeline.push_audio(&[0; 320]).unwrap();
				}
				drop(pipeline);
			}
		});
		let mut probes = 0;
		let mut maximum_probe_ms = 0;
		while !worker.is_finished() {
			let start = Instant::now();
			let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(1)).unwrap();
			stream
				.set_read_timeout(Some(Duration::from_secs(1)))
				.unwrap();
			stream
				.write_all(b"GET /live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
				.unwrap();
			let mut response = String::new();
			stream.read_to_string(&mut response).unwrap();
			assert!(response.starts_with("HTTP/1.1 200"));
			maximum_probe_ms = maximum_probe_ms.max(start.elapsed().as_millis());
			probes += 1;
			std::thread::sleep(Duration::from_millis(10));
		}
		runtime.block_on(worker).unwrap();
		server.abort();
		runtime.shutdown_timeout(Duration::from_secs(1));
		assert!(probes > 0);
		println!("Health probes: {probes}; max latency: {maximum_probe_ms}ms");
		println!("Native call diagnostics: {}", snapshot());
	}
}
