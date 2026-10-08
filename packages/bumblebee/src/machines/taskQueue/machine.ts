import { setup, assign, enqueueActions, fromPromise } from "xstate";
import type { Actor } from "xstate";
import { produce } from "immer";
import type { TaskQueueTask, TaskQueueContext, TaskQueueEvents } from "./types";
import { timeout } from "../../utils/timeout";
import { v4 as uuidv4 } from "uuid";

type TaskQueueActor = Actor<typeof machine>;
type QueueAddEvent = Extract<TaskQueueEvents, { type: "queue.add" }>;

const taskKindRank = (kind: TaskQueueTask["kind"]) => {
	if (kind === "interrupt") return 3;
	if (kind === "voice") return 2;
	return 1;
};

const errorMessage = (error: unknown) => (error instanceof Error ? error.message : String(error));

const taskParamsFromQueueAdd = (event: QueueAddEvent): Omit<TaskQueueTask, "id"> => ({
	kind: event.kind ?? "chat",
	priority: event.priority ?? 0,
	preempt: event.preempt,
	expireAfter: event.expireAfter,
	abortSignal: event.abortSignal,
	exec: event.exec
});

const machine = setup({
	types: {
		context: {} as TaskQueueContext,
		events: {} as TaskQueueEvents
	},
	guards: {
		hasTasks: ({ context }) => context.queue.length > 0,
		isEmpty: ({ context }) => context.queue.length === 0,
		hasEligibleTasks: ({ context }) =>
			context.queue.some((task) => {
				if (task.kind === "interrupt") return true;
				// Voice interactions stay responsive even while pauseAll is active.
				if (task.kind === "voice") return true;
				if (context.pauseAll) return false;
				return !context.pauseNonVoice;
			}),
		isEventEligible: ({ context, event }) => {
			if (event.type !== "queue.add") return false;
			const kind = event.kind ?? "chat";
			if (kind === "interrupt") return true;
			if (kind === "voice") return true;
			if (context.pauseAll) return false;
			return !context.pauseNonVoice;
		},
		shouldPreempt: ({ context, event }) => {
			if (event.type !== "queue.add") return false;
			if (event.preempt === false) return false;
			if (!context.currentTask) return false;
			const incomingPriority = event.priority ?? 0;
			const kind = event.kind ?? "chat";
			if (kind === "interrupt") {
				if (context.currentTask.kind !== "interrupt") return true;
				return incomingPriority > context.currentTask.priority;
			}
			// Voice tasks should preempt non-voice playback immediately, even at equal priority.
			if (kind === "voice") {
				if (context.currentTask.kind === "interrupt") return false;
				if (context.currentTask.kind !== "voice") return true;
				return incomingPriority > context.currentTask.priority;
			}
			if (incomingPriority <= context.currentTask.priority) return false;
			if (context.pauseAll) return false;
			return !context.pauseNonVoice;
		}
	},
	actions: {
		enqueueTask: enqueueActions(({ enqueue, self }, params: Omit<TaskQueueTask, "id">) => {
			enqueue.assign(({ context }) =>
				produce(context, (ctx) => {
					const { expireAfter, abortSignal, exec, kind, preempt, priority } = params;
					const abortController = new AbortController();
					const task = {
						id: uuidv4(),
						kind,
						priority,
						preempt,
						expireAfter,
						abortSignal,
						exec,
						abortController
					} satisfies TaskQueueTask & { abortController: AbortController };
					ctx.queue.push(task);
					ctx.lastError = undefined;
					if (expireAfter != null) {
						enqueue.spawnChild("monitorExpiration", {
							id: `task-expire-${task.id}`,
							input: { task, parent: self as TaskQueueActor }
						});
					}
					if (abortSignal) {
						enqueue.spawnChild("monitorAbort", {
							id: `task-abort-${task.id}`,
							input: { task, parent: self as TaskQueueActor }
						});
					}
				})
			);
		}),
		dequeueTask: enqueueActions(({ enqueue }, params: { taskId: string }) => {
			enqueue.assign(({ context }) =>
				produce(context, ({ queue }) => {
					const index = queue.findIndex((task) => task.id === params.taskId);
					if (index !== -1) {
						queue.splice(index, 1);
					}
				})
			);
			enqueue.stopChild(`task-expire-${params.taskId}`);
			enqueue.stopChild(`task-abort-${params.taskId}`);
		}),
		clearQueue: enqueueActions(({ enqueue, context }) => {
			for (const task of context.queue) {
				enqueue.stopChild(`task-expire-${task.id}`);
				enqueue.stopChild(`task-abort-${task.id}`);
			}
			enqueue.assign({ queue: [] });
		}),
		setCurrentTask: assign(({ context }) =>
			produce(context, (ctx) => {
				const eligibleIndexes = ctx.queue
					.map((task, index) => ({ task, index }))
					.filter(({ task }) => {
						if (task.kind === "interrupt") return true;
						if (task.kind === "voice") return true;
						if (ctx.pauseAll) return false;
						return !ctx.pauseNonVoice;
					});
				if (eligibleIndexes.length === 0) {
					ctx.currentTask = undefined;
					return;
				}
				eligibleIndexes.sort((a, b) => {
					const kindRankDiff = taskKindRank(b.task.kind) - taskKindRank(a.task.kind);
					if (kindRankDiff !== 0) return kindRankDiff;
					if (a.task.priority !== b.task.priority) {
						return b.task.priority - a.task.priority;
					}
					return a.index - b.index;
				});
				const selected = eligibleIndexes[0];
				if (!selected) {
					ctx.currentTask = undefined;
					return;
				}
				ctx.currentTask = selected.task;
				ctx.queue.splice(selected.index, 1);
			})
		),
		clearCurrentTask: enqueueActions(({ enqueue, context }) => {
			const currentTask = context.currentTask;
			if (currentTask) {
				enqueue.stopChild(`task-expire-${currentTask.id}`);
				enqueue.stopChild(`task-abort-${currentTask.id}`);
			}
			enqueue.assign(({ context }) =>
				produce(context, (ctx) => {
					// XState assign merges partial updates; deleting a key from the returned object
					// does not reliably remove it from context. Explicitly unset instead.
					ctx.currentTask = undefined;
				})
			);
		}),
		recordTaskError: assign(({ context, event }) =>
			produce(context, (ctx) => {
				ctx.lastError = "error" in event ? errorMessage(event.error) : "Task execution failed";
			})
		),
		abortCurrentTask: ({ context }) => {
			context.currentTask?.abortController?.abort("queue.clear");
		},
		pauseQueue: assign({ pauseNonVoice: true }),
		resumeQueue: assign({ pauseNonVoice: false }),
		pauseAllQueue: assign({ pauseAll: true }),
		resumeAllQueue: assign({ pauseAll: false }),
		preemptCurrentTask: assign(({ context }) =>
			produce(context, (ctx) => {
				const current = ctx.currentTask as TaskQueueTask | undefined;
				if (!current) return;
				// Abort the in-flight attempt. Replayable playback can go back into the queue, but
				// voice interactions represent a live microphone turn and must not restart later
				// with stale captured audio.
				const prev = current.abortController;
				prev?.abort("preempt");
				if (current.kind !== "voice") {
					current.abortController = new AbortController();
					ctx.queue.push(current);
				}
				// See note in clearCurrentTask: explicitly unset instead of delete.
				ctx.currentTask = undefined;
			})
		)
	},
	actors: {
		monitorExpiration: fromPromise<
			void,
			{ task: TaskQueueTask; parent: TaskQueueActor },
			TaskQueueEvents
		>(async ({ input, signal }) => {
			await timeout(input.task.expireAfter!, signal);
			input.parent.send({ type: "queue.remove", taskId: input.task.id });
		}),
		monitorAbort: fromPromise<
			void,
			{ task: TaskQueueTask; parent: TaskQueueActor },
			TaskQueueEvents
		>(async ({ input, signal }) => {
			await new Promise<void>((resolve) => {
				if (input.task.abortSignal!.aborted) {
					resolve();
					return;
				}
				input.task.abortSignal!.addEventListener("abort", () => resolve(), { once: true, signal });
			});
			input.parent.send({ type: "queue.remove", taskId: input.task.id });
		}),
		runTask: fromPromise<string, TaskQueueTask, TaskQueueEvents>(
			async ({ input: task, emit, signal }) => {
				const { abortController } = task as TaskQueueTask & { abortController: AbortController };
				const abortFromExternal = () =>
					abortController.abort(task.abortSignal?.reason ?? "user supplied abort signal");
				if (task.abortSignal?.aborted) {
					abortFromExternal();
				} else if (task.abortSignal) {
					task.abortSignal.addEventListener("abort", abortFromExternal, { once: true });
				}
				abortController.signal.addEventListener(
					"abort",
					() => {
						if (abortController.signal.reason === "preempt") return;
						emit({ type: "queue.remove", taskId: task.id });
					},
					{ once: true }
				);
				try {
					// Give higher-priority tasks a chance to preempt before executing.
					await Promise.resolve();
					if (signal.aborted || abortController.signal.aborted) {
						return task.id;
					}
					await task.exec(abortController.signal);
					return task.id;
				} finally {
					task.abortSignal?.removeEventListener("abort", abortFromExternal);
				}
			}
		)
	}
}).createMachine({
	context: {
		queue: [],
		pauseNonVoice: false,
		pauseAll: false
	},
	initial: "idle",
	on: {
		"queue.remove": {
			actions: {
				type: "dequeueTask",
				params: ({ event }) => ({ taskId: event.taskId })
			}
		},
		"queue.clear": {
			actions: ["abortCurrentTask", "clearQueue", "clearCurrentTask"],
			target: ".idle"
		},
		"queue.stopCurrent": {
			actions: "abortCurrentTask"
		},
		"queue.pause": {
			actions: "pauseQueue"
		},
		"queue.resume": {
			actions: "resumeQueue"
		},
		"queue.pauseAll": {
			actions: "pauseAllQueue"
		},
		"queue.resumeAll": {
			actions: "resumeAllQueue"
		}
	},
	states: {
		idle: {
			always: [
				{ guard: "hasEligibleTasks", target: "processing" },
				{ guard: "hasTasks", target: "paused" }
			],
			on: {
				"queue.add": [
					{
						guard: "isEventEligible",
						actions: {
							type: "enqueueTask",
							params: ({ event }) => taskParamsFromQueueAdd(event)
						},
						target: "processing"
					},
					{
						actions: {
							type: "enqueueTask",
							params: ({ event }) => taskParamsFromQueueAdd(event)
						},
						target: "paused"
					}
				]
			}
		},
		paused: {
			always: [
				{ guard: "hasEligibleTasks", target: "processing" },
				{ guard: "isEmpty", target: "idle" }
			],
			on: {
				"queue.add": [
					{
						guard: "isEventEligible",
						actions: {
							type: "enqueueTask",
							params: ({ event }) => taskParamsFromQueueAdd(event)
						},
						target: "processing"
					},
					{
						actions: {
							type: "enqueueTask",
							params: ({ event }) => taskParamsFromQueueAdd(event)
						}
					}
				],
				"queue.resume": [
					{ guard: "hasEligibleTasks", actions: "resumeQueue", target: "processing" },
					{ guard: "isEmpty", actions: "resumeQueue", target: "idle" },
					{ actions: "resumeQueue" }
				],
				"queue.resumeAll": [
					{ guard: "hasEligibleTasks", actions: "resumeAllQueue", target: "processing" },
					{ guard: "isEmpty", actions: "resumeAllQueue", target: "idle" },
					{ actions: "resumeAllQueue" }
				]
			}
		},
		processing: {
			entry: "setCurrentTask",
			invoke: {
				id: "runningTask",
				src: "runTask",
				input: ({ context }) => context.currentTask!,
				onDone: [
					{ actions: "clearCurrentTask", guard: "isEmpty", target: "idle" },
					{
						actions: "clearCurrentTask",
						guard: "hasEligibleTasks",
						target: "processing",
						reenter: true
					},
					{ actions: "clearCurrentTask", guard: "hasTasks", target: "paused" }
				],
				onError: [
					{ actions: ["recordTaskError", "clearCurrentTask"], guard: "isEmpty", target: "idle" },
					{
						actions: ["recordTaskError", "clearCurrentTask"],
						guard: "hasEligibleTasks",
						target: "processing",
						reenter: true
					},
					{ actions: ["recordTaskError", "clearCurrentTask"], guard: "hasTasks", target: "paused" }
				]
			},
			on: {
				"queue.add": [
					{
						guard: "shouldPreempt",
						actions: [
							"preemptCurrentTask",
							{
								type: "enqueueTask",
								params: ({ event }) => taskParamsFromQueueAdd(event)
							}
						],
						target: "processing",
						reenter: true
					},
					{
						actions: {
							type: "enqueueTask",
							params: ({ event }) => taskParamsFromQueueAdd(event)
						}
					}
				]
			}
		}
	}
});

export { machine };
