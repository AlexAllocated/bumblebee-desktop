import { describe, expect, mock, test } from "bun:test";
import { OverlayEventEmitter } from "./events";
import type { ToolPresentationPayload } from "./toolPresentation";

const waitForMicrotask = () => new Promise((resolve) => queueMicrotask(resolve));

const startedPresentationPayload = (id: string): ToolPresentationPayload => ({
	type: "tool.presentation.started",
	presentationId: id,
	tool: "researchWeb",
	title: "Web research",
	requestText: "cats",
	startedAtMs: 1
});

const completedPresentationPayload = (id: string): ToolPresentationPayload =>
	({
		type: "tool.presentation.completed",
		presentationId: id,
		tool: "researchWeb",
		title: "Web research",
		requestText: "cats",
		completedAtMs: 2,
		content: {
			kind: "web_document",
			title: "Search summary",
			summary: "Cats",
			points: [],
			sources: [],
			generatedAtMs: 2
		}
	}) satisfies ToolPresentationPayload;

describe("OverlayEventEmitter", () => {
	test("isolates throwing handlers from later handlers", () => {
		const originalWarn = console.warn;
		const warnings: unknown[][] = [];
		console.warn = (...args: unknown[]) => {
			warnings.push(args);
		};
		try {
			const events = new OverlayEventEmitter();
			const laterHandler = mock(() => {});

			events.on("speech:start", () => {
				throw new Error("handler failed");
			});
			events.on("speech:start", laterHandler);

			expect(() =>
				events.emit({
					type: "speech:start",
					audioId: "audio-1",
					target: { type: "bumblebee" }
				})
			).not.toThrow();
			expect(laterHandler).toHaveBeenCalledTimes(1);
			expect(warnings[0]?.[0]).toBe("Overlay event handler failed for speech:start:");
		} finally {
			console.warn = originalWarn;
		}
	});

	test("replays recent tool presentation events to late subscribers", async () => {
		const events = new OverlayEventEmitter();
		const replayedEvents: Array<{ type: "tool:presentation"; payload: ToolPresentationPayload }> =
			[];

		events.emit({
			type: "tool:presentation",
			payload: startedPresentationPayload("presentation-1")
		});
		events.emit({
			type: "tool:presentation",
			payload: completedPresentationPayload("presentation-1")
		});

		events.on(
			"tool:presentation",
			(event) => {
				replayedEvents.push(event);
			},
			{ replay: true }
		);
		await waitForMicrotask();

		expect(replayedEvents).toHaveLength(2);
		expect(replayedEvents.map((event) => event.payload.type)).toEqual([
			"tool.presentation.started",
			"tool.presentation.completed"
		]);
	});

	test("does not replay general overlay events", async () => {
		const events = new OverlayEventEmitter();
		const handler = mock(() => {});

		events.emit({
			type: "speech:start",
			audioId: "audio-1",
			target: { type: "bumblebee" }
		});

		events.on("speech:start", handler, { replay: true });
		await waitForMicrotask();

		expect(handler).not.toHaveBeenCalled();
	});

	test("does not replay after unsubscribe or clear", async () => {
		const events = new OverlayEventEmitter();
		const unsubscribedHandler = mock(() => {});
		const clearedHandler = mock(() => {});

		events.emit({
			type: "tool:presentation",
			payload: startedPresentationPayload("presentation-1")
		});

		const unsubscribe = events.on("tool:presentation", unsubscribedHandler, { replay: true });
		unsubscribe();
		await waitForMicrotask();
		expect(unsubscribedHandler).not.toHaveBeenCalled();

		events.clear();
		events.on("tool:presentation", clearedHandler, { replay: true });
		await waitForMicrotask();
		expect(clearedHandler).not.toHaveBeenCalled();
	});
});
