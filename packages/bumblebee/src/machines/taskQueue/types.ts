type TaskQueueTask = {
	id: string;
	// interrupt tasks bypass pause gates (pauseAll / pauseNonVoice) and may preempt playback.
	kind: "chat" | "event" | "voice" | "interrupt";
	priority: number;
	preempt?: boolean;
	expireAfter?: number;
	abortSignal?: AbortSignal;
	exec: (abortSignal: AbortSignal) => Promise<void>;
	abortController?: AbortController;
};

type TaskQueueContext = {
	queue: TaskQueueTask[];
	currentTask?: TaskQueueTask;
	lastError?: string;
	pauseNonVoice: boolean;
	pauseAll: boolean;
};

type TaskQueueEvents =
	| {
			type: "queue.add";
			kind?: TaskQueueTask["kind"];
			priority?: number;
			preempt?: boolean;
			expireAfter?: number;
			abortSignal?: AbortSignal;
			exec: TaskQueueTask["exec"];
	  }
	| { type: "queue.remove"; taskId: string }
	| { type: "queue.clear" }
	| { type: "queue.stopCurrent" }
	| { type: "queue.pause" }
	| { type: "queue.resume" }
	| { type: "queue.pauseAll" }
	| { type: "queue.resumeAll" };

export type { TaskQueueTask, TaskQueueContext, TaskQueueEvents };
