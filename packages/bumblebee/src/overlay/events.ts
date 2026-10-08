import type {
	EventUnsubscribe,
	OverlayEvent,
	OverlayEventFor,
	OverlayEventSubscriptionOptions,
	OverlayEventType
} from "./types";

const REPLAYABLE_EVENT_TYPES = new Set<OverlayEventType>(["tool:presentation"]);
const REPLAY_LIMIT = 32;
const REPLAY_TTL_MS = 60_000;

export class OverlayEventEmitter {
	#handlers = new Map<OverlayEventType, Set<(event: OverlayEvent) => void>>();
	#replay = new Map<OverlayEventType, { event: OverlayEvent; receivedAtMs: number }[]>();

	on<T extends OverlayEventType>(
		event: T,
		handler: (event: OverlayEventFor<T>) => void,
		options?: OverlayEventSubscriptionOptions
	): EventUnsubscribe {
		const handlers = this.#handlers.get(event) ?? new Set<(event: OverlayEvent) => void>();
		const overlayHandler = handler as (event: OverlayEvent) => void;
		handlers.add(overlayHandler);
		this.#handlers.set(event, handlers);
		if (options?.replay) {
			const replayEvents = this.#replayableEvents(event);
			if (replayEvents.length > 0) {
				queueMicrotask(() => {
					if (!handlers.has(overlayHandler)) return;
					for (const replayEvent of replayEvents) {
						if (!handlers.has(overlayHandler)) return;
						this.#dispatchToHandler(event, overlayHandler, replayEvent);
					}
				});
			}
		}
		return () => handlers.delete(overlayHandler);
	}

	emit(event: OverlayEvent) {
		this.#remember(event);
		for (const handler of this.#handlers.get(event.type) ?? []) {
			this.#dispatchToHandler(event.type, handler, event);
		}
	}

	clear() {
		this.#handlers.clear();
		this.#replay.clear();
	}

	#remember(event: OverlayEvent) {
		if (!REPLAYABLE_EVENT_TYPES.has(event.type)) return;
		const now = Date.now();
		const events = this.#freshReplayEntries(event.type, now);
		events.push({ event, receivedAtMs: now });
		this.#replay.set(event.type, events.slice(-REPLAY_LIMIT));
	}

	#replayableEvents<T extends OverlayEventType>(event: T, now = Date.now()) {
		return this.#freshReplayEntries(event, now).map((entry) => entry.event as OverlayEventFor<T>);
	}

	#freshReplayEntries(event: OverlayEventType, now = Date.now()) {
		const events = this.#replay.get(event) ?? [];
		const fresh = events.filter((entry) => now - entry.receivedAtMs <= REPLAY_TTL_MS);
		if (fresh.length !== events.length) this.#replay.set(event, fresh);
		return fresh;
	}

	#dispatchToHandler(
		eventType: OverlayEventType,
		handler: (event: OverlayEvent) => void,
		event: OverlayEvent
	) {
		try {
			handler(event);
		} catch (error) {
			console.warn(`Overlay event handler failed for ${eventType}:`, error);
		}
	}
}
