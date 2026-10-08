import { describe, expect, test } from "bun:test";
import {
	parseOverlayMediaPlayPayload,
	parseOverlayMediaStartPayload,
	parseOverlayMediaStopPayload
} from "./media";

describe("parseOverlayMediaPlayPayload", () => {
	test("parses a valid video payload", () => {
		expect(
			parseOverlayMediaPlayPayload({
				type: "overlay.media.play",
				mediaId: "kfc-bumblebee",
				kind: "video",
				url: "https://example.com/video.mp4?sig=123",
				contentType: "video/mp4",
				playbackId: "playback-1",
				muted: true,
				startPaused: true,
				volume: 0.75,
				displayMs: 30_000,
				widthViewportPercent: 20,
				startedAtMs: 123
			})
		).toEqual({
			type: "overlay.media.play",
			mediaId: "kfc-bumblebee",
			kind: "video",
			url: "https://example.com/video.mp4?sig=123",
			contentType: "video/mp4",
			playbackId: "playback-1",
			muted: true,
			startPaused: true,
			volume: 0.75,
			displayMs: 30_000,
			widthViewportPercent: 20,
			startedAtMs: 123
		});
	});

	test("clamps video volume to the media element range", () => {
		expect(
			parseOverlayMediaPlayPayload({
				type: "overlay.media.play",
				mediaId: "kfc-bumblebee",
				kind: "video",
				url: "https://example.com/video.mp4",
				contentType: "video/mp4",
				volume: 2
			})?.volume
		).toBe(1);

		expect(
			parseOverlayMediaPlayPayload({
				type: "overlay.media.play",
				mediaId: "kfc-bumblebee",
				kind: "video",
				url: "https://example.com/video.mp4",
				contentType: "video/mp4",
				volume: -1
			})?.volume
		).toBe(0);
	});

	test("rejects unsupported or unsafe payloads", () => {
		expect(parseOverlayMediaPlayPayload({ type: "overlay.media.play" })).toBeNull();
		expect(
			parseOverlayMediaPlayPayload({
				type: "overlay.media.play",
				mediaId: "kfc-bumblebee",
				kind: "video",
				url: "javascript:alert(1)",
				contentType: "video/mp4"
			})
		).toBeNull();
		expect(
			parseOverlayMediaPlayPayload({
				type: "overlay.media.play",
				mediaId: "kfc-bumblebee",
				kind: "audio",
				url: "https://example.com/audio.ogg",
				contentType: "audio/ogg"
			})
		).toBeNull();
	});
});

describe("parseOverlayMediaStartPayload", () => {
	test("parses a valid start payload", () => {
		expect(
			parseOverlayMediaStartPayload({
				type: "overlay.media.start",
				playbackId: "playback-1",
				startedAtMs: 456
			})
		).toEqual({
			type: "overlay.media.start",
			playbackId: "playback-1",
			startedAtMs: 456
		});
	});

	test("rejects missing playback ids", () => {
		expect(parseOverlayMediaStartPayload({ type: "overlay.media.start" })).toBeNull();
	});
});

describe("parseOverlayMediaStopPayload", () => {
	test("parses a valid stop payload", () => {
		expect(
			parseOverlayMediaStopPayload({
				type: "overlay.media.stop",
				playbackId: "playback-1"
			})
		).toEqual({
			type: "overlay.media.stop",
			playbackId: "playback-1"
		});
	});

	test("rejects missing playback ids", () => {
		expect(parseOverlayMediaStopPayload({ type: "overlay.media.stop" })).toBeNull();
	});
});
