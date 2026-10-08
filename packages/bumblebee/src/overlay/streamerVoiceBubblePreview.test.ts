import { describe, expect, mock, test } from "bun:test";
import type {
	BubbleLayer,
	BubbleRevealOptions,
	BubbleShowOptions,
	BubbleUpdateOptions
} from "@hivetech/speech-bubbles";
import { StreamerVoiceBubblePreview } from "./streamerVoiceBubblePreview";

class FakeBubbleLayer implements BubbleLayer {
	readonly sessions = [];
	readonly getPage = mock((_id: string) => null);
	readonly show = mock((_options: BubbleShowOptions) => {});
	readonly reveal = mock((_id: string, _text: string, _options?: BubbleRevealOptions) => {});
	readonly update = mock((_id: string, _options: BubbleUpdateOptions) => {});
	readonly hide = mock(async (_id: string, _options?: { immediate?: boolean }) => {});
	readonly setPaused = mock((_paused: boolean) => {});
	readonly dispose = mock(() => {});
}

const previewOptions = () => ({
	pose: {
		anchor: { x: 120, y: 80 },
		tailVector: { x: 32, y: -24 },
		subjectRadius: 48
	},
	style: {
		background: "#fff8cc",
		border: "#221100",
		text: "#110800",
		shadow: "rgba(0,0,0,0.2)",
		tail: "center" as const,
		fontFamily: "rounded" as const,
		shape: "cloud" as const
	},
	scale: 1.25,
	maxWidthPercent: 42,
	maxHeightPercent: 28,
	fixedBodySize: { width: 420, height: 180 },
	fixedBodyRect: { left: 40, top: 60, width: 420, height: 180 }
});

describe("StreamerVoiceBubblePreview", () => {
	test("shows and refreshes one detached bubble with authored geometry", () => {
		const layer = new FakeBubbleLayer();
		const createLayer = mock(() => layer);
		const preview = new StreamerVoiceBubblePreview({ createLayer, zIndex: 500 });

		preview.show(previewOptions());
		preview.refresh({
			...previewOptions(),
			pose: { anchor: { x: 140, y: 90 }, tailVector: { x: 24, y: -30 }, subjectRadius: 52 }
		});

		expect(createLayer).toHaveBeenCalledWith({ zIndex: 500, debug: false });
		expect(layer.show).toHaveBeenCalledTimes(1);
		expect(layer.update).toHaveBeenCalledTimes(1);
		expect(layer.show.mock.calls[0]?.[0]).toMatchObject({
			id: "streamer-voice-bubble:preview",
			text: "Streamer voice bubble",
			queueKey: "subject:streamerVoiceBubblePreview",
			placeholder: true,
			zIndex: 501,
			fixedBodySize: { width: 420, height: 180 },
			fixedBodyRect: { left: 40, top: 60, width: 420, height: 180 }
		});
		expect(layer.update.mock.calls[0]?.[1]).toMatchObject({
			pose: { anchor: { x: 140, y: 90 }, tailVector: { x: 24, y: -30 }, subjectRadius: 52 },
			fixedBodyRect: { left: 40, top: 60, width: 420, height: 180 }
		});
	});

	test("hides and disposes the detached layer without requiring caller cleanup", async () => {
		const layer = new FakeBubbleLayer();
		const preview = new StreamerVoiceBubblePreview({ createLayer: () => layer });

		preview.show(previewOptions());
		await preview.dispose();

		expect(layer.hide).toHaveBeenCalledWith("streamer-voice-bubble:preview", { immediate: true });
		expect(layer.dispose).toHaveBeenCalledTimes(1);
	});
});
