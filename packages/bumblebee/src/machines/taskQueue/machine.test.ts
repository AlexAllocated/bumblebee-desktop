import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createActor } from "xstate";
import { taskQueueMachine } from "./index";

let actor: ReturnType<typeof createActor<typeof taskQueueMachine>>;

const releaseBlockedTask = (release: (() => void) | null) => {
	release?.();
};

const monitorChildIds = () =>
	Object.keys(actor.getSnapshot().children).filter(
		(id) => id.startsWith("task-expire-") || id.startsWith("task-abort-")
	);

beforeEach(() => {
	actor = createActor(taskQueueMachine);
	actor.start();
});

afterEach(() => {
	actor.stop();
});

describe("taskQueueMachine", () => {
	test("skips tasks whose caller signal was already aborted", async () => {
		const controller = new AbortController();
		controller.abort("stale");
		let ran = false;

		actor.send({
			type: "queue.add",
			abortSignal: controller.signal,
			exec: async () => {
				ran = true;
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 25));

		const snapshot = actor.getSnapshot();
		expect(ran).toBe(false);
		expect(snapshot.context.currentTask).toBeUndefined();
		expect(snapshot.context.queue).toHaveLength(0);
		expect(snapshot.value).toBe("idle");
	});

	test("records task execution errors and continues queued work", async () => {
		const completions: string[] = [];

		const done = new Promise<void>((resolve) => {
			actor.subscribe((snapshot) => {
				if (
					snapshot.value === "idle" &&
					snapshot.context.queue.length === 0 &&
					completions.length === 1
				) {
					resolve();
				}
			});
		});

		actor.send({
			type: "queue.add",
			exec: async () => {
				throw new Error("boom");
			}
		});

		actor.send({
			type: "queue.add",
			exec: async () => {
				completions.push("second");
			}
		});

		await done;

		const snapshot = actor.getSnapshot();
		expect(completions).toEqual(["second"]);
		expect(snapshot.context.currentTask).toBeUndefined();
		expect(snapshot.context.queue).toHaveLength(0);
		expect(snapshot.context.lastError).toBe("boom");
	});

	test("can prioritize queued tasks without preempting the active task", async () => {
		const completed: string[] = [];
		let unblockActive: (() => void) | null = null;

		const done = new Promise<void>((resolve) => {
			actor.subscribe((snapshot) => {
				if (
					snapshot.value === "idle" &&
					snapshot.context.queue.length === 0 &&
					completed.length === 3
				) {
					resolve();
				}
			});
		});

		actor.send({
			type: "queue.add",
			kind: "chat",
			priority: 0,
			exec: async () => {
				await new Promise<void>((resolve) => {
					unblockActive = resolve;
				});
				completed.push("active");
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 10));
		actor.send({
			type: "queue.add",
			kind: "chat",
			priority: 0,
			exec: async () => {
				completed.push("low");
			}
		});
		actor.send({
			type: "queue.add",
			kind: "event",
			priority: 100,
			preempt: false,
			exec: async () => {
				completed.push("high");
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 25));
		expect(completed).toEqual([]);

		releaseBlockedTask(unblockActive);
		await done;
		expect(completed).toEqual(["active", "high", "low"]);
	});

	test("pauseAll blocks playback but still lets voice tasks run", async () => {
		const completed: string[] = [];

		actor.send({ type: "queue.pauseAll" });
		actor.send({
			type: "queue.add",
			kind: "chat",
			exec: async () => {
				completed.push("chat");
			}
		});
		actor.send({
			type: "queue.add",
			kind: "voice",
			exec: async () => {
				completed.push("voice");
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 25));
		expect(completed).toEqual(["voice"]);

		actor.send({ type: "queue.resumeAll" });
		await new Promise((resolve) => setTimeout(resolve, 25));
		expect(completed).toEqual(["voice", "chat"]);
	});

	test("voice tasks preempt active non-voice playback", async () => {
		const completed: string[] = [];
		let unblockChat: (() => void) | null = null;

		actor.send({
			type: "queue.add",
			kind: "chat",
			exec: async (signal) => {
				await new Promise<void>((resolve) => {
					unblockChat = resolve;
					signal.addEventListener("abort", () => resolve(), { once: true });
				});
				if (signal.aborted) return;
				completed.push("chat");
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 10));
		actor.send({
			type: "queue.add",
			kind: "voice",
			priority: 1,
			exec: async () => {
				completed.push("voice");
			}
		});

		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(completed[0]).toBe("voice");
		expect(completed).not.toContain("chat");

		releaseBlockedTask(unblockChat);
		await new Promise((resolve) => setTimeout(resolve, 50));
		expect(completed).toContain("chat");
	});

	test("stops per-task monitor children after successful execution", async () => {
		const controller = new AbortController();
		const done = new Promise<void>((resolve) => {
			actor.subscribe((snapshot) => {
				if (snapshot.value === "idle" && snapshot.context.queue.length === 0) {
					resolve();
				}
			});
		});

		actor.send({
			type: "queue.add",
			expireAfter: 10_000,
			abortSignal: controller.signal,
			exec: async () => {}
		});

		expect(monitorChildIds()).not.toHaveLength(0);
		await done;
		expect(monitorChildIds()).toHaveLength(0);
	});
});
