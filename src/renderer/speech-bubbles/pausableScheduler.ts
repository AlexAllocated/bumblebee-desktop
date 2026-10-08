export type PausableTimer = {
	readonly id: number;
};

type ScheduledTask = {
	handle: PausableTimer;
	callback: () => void;
	remainingMs: number;
	startedAtMs: number;
	timer: ReturnType<typeof setTimeout> | null;
};

const monotonicNow = () =>
	typeof performance !== "undefined" && typeof performance.now === "function"
		? performance.now()
		: Date.now();

/**
 * A timeout scheduler whose clock stops while playback is paused.
 *
 * Presentation code should schedule visual lifecycle work here instead of
 * independently compensating every timeout after a pause.
 */
export class PausableScheduler {
	#nextId = 1;
	#paused = false;
	#tasks = new Map<number, ScheduledTask>();
	#pausedAtMs: number | null = null;
	#totalPausedMs = 0;

	get paused() {
		return this.#paused;
	}

	now() {
		const current = this.#pausedAtMs ?? monotonicNow();
		return current - this.#totalPausedMs;
	}

	setTimeout(callback: () => void, delayMs: number): PausableTimer {
		const handle = { id: this.#nextId++ };
		const task: ScheduledTask = {
			handle,
			callback,
			remainingMs: Math.max(0, Number.isFinite(delayMs) ? delayMs : 0),
			startedAtMs: monotonicNow(),
			timer: null
		};
		this.#tasks.set(handle.id, task);
		if (!this.#paused) this.#arm(task);
		return handle;
	}

	clearTimeout(handle: PausableTimer | null | undefined) {
		if (!handle) return;
		const task = this.#tasks.get(handle.id);
		if (!task) return;
		if (task.timer) clearTimeout(task.timer);
		this.#tasks.delete(handle.id);
	}

	setPaused(paused: boolean) {
		if (paused === this.#paused) return;
		const current = monotonicNow();
		this.#paused = paused;
		if (paused) {
			this.#pausedAtMs = current;
			for (const task of this.#tasks.values()) {
				if (!task.timer) continue;
				clearTimeout(task.timer);
				task.timer = null;
				task.remainingMs = Math.max(0, task.remainingMs - (current - task.startedAtMs));
			}
			return;
		}

		if (this.#pausedAtMs !== null) this.#totalPausedMs += current - this.#pausedAtMs;
		this.#pausedAtMs = null;
		for (const task of this.#tasks.values()) this.#arm(task);
	}

	dispose() {
		for (const task of this.#tasks.values()) {
			if (task.timer) clearTimeout(task.timer);
		}
		this.#tasks.clear();
	}

	#arm(task: ScheduledTask) {
		if (this.#paused || !this.#tasks.has(task.handle.id)) return;
		task.startedAtMs = monotonicNow();
		task.timer = setTimeout(() => {
			task.timer = null;
			if (!this.#tasks.delete(task.handle.id)) return;
			task.callback();
		}, task.remainingMs);
	}
}
