import { describe, expect, test } from "bun:test";
import {
	createBumblebeeTimeline,
	createBumblebeeTimelineBuilder,
	sampleBumblebeeMove,
	sampleBumblebeeTimeline,
	sampleSpeechTimeline,
	type BumblebeeMoveAction,
	type PreparedSpeechAsset
} from "./timeline";

const speech: PreparedSpeechAsset = {
	id: "speech-1",
	text: "Hello Bumblebee",
	audioDurationMs: 1000,
	timeline: {
		words: [
			{ offsetMs: 0, durationMs: 400, text: "Hello", textOffset: 0, wordLength: 5 },
			{ offsetMs: 500, durationMs: 300, text: "Bumblebee", textOffset: 6, wordLength: 9 }
		],
		visemes: [
			{ offsetMs: 0, visemeId: 0 },
			{ offsetMs: 250, visemeId: 2 },
			{ offsetMs: 800, visemeId: 21 }
		]
	}
};

describe("Bumblebee timeline", () => {
	test("samples move actions deterministically", () => {
		const action: BumblebeeMoveAction = {
			type: "moveTo",
			startMs: 100,
			durationMs: 1000,
			from: { position: { x: 0, y: 0 }, scale: 0.5 },
			to: { position: { x: 1, y: 0.5 }, scale: 1 },
			easing: "linear"
		};

		expect(sampleBumblebeeMove(action, 100)).toEqual({
			position: { x: 0, y: 0 },
			scale: 0.5
		});
		expect(sampleBumblebeeMove(action, 600)).toEqual({
			position: { x: 0.5, y: 0.25 },
			scale: 0.75
		});
		expect(sampleBumblebeeMove(action, 1100)).toEqual({
			position: { x: 1, y: 0.5 },
			scale: 1
		});
	});

	test("samples speech reveal and viseme state by elapsed time", () => {
		expect(sampleSpeechTimeline(speech, 100)).toMatchObject({
			active: true,
			revealedText: "Hello",
			currentWord: speech.timeline!.words[0],
			currentViseme: speech.timeline!.visemes[0],
			mouthOpen: 0
		});
		expect(sampleSpeechTimeline(speech, 300)).toMatchObject({
			active: true,
			revealedText: "Hello",
			currentWord: speech.timeline!.words[0],
			currentViseme: speech.timeline!.visemes[1],
			mouthOpen: 1
		});
		expect(sampleSpeechTimeline(speech, 900)).toMatchObject({
			active: true,
			revealedText: "Hello Bumblebee",
			currentWord: null,
			currentViseme: speech.timeline!.visemes[2],
			mouthOpen: 0.06
		});
	});

	test("builds and samples an action timeline", () => {
		const timeline = createBumblebeeTimelineBuilder({
			initialPose: { position: { x: 0.5, y: 0.7 }, scale: 0.4 }
		})
			.stance("flying")
			.moveTo({ position: { x: 0.5, y: 0.4 }, scale: 0.7 }, { durationMs: 1000, easing: "linear" })
			.say(speech)
			.build({ paddingMs: 500 });

		expect(timeline.durationMs).toBe(2500);
		expect(sampleBumblebeeTimeline(timeline, 500)).toMatchObject({
			pose: { position: { x: 0.5, y: 0.55 }, scale: 0.55 },
			stance: "flying",
			talking: false
		});
		expect(sampleBumblebeeTimeline(timeline, 1300)).toMatchObject({
			pose: { position: { x: 0.5, y: 0.4 }, scale: 0.7 },
			talking: true,
			mouthOpen: 1
		});
	});

	test("sorts direct timeline actions and derives duration", () => {
		const timeline = createBumblebeeTimeline([
			{ type: "say", startMs: 200, speech },
			{ type: "stance", startMs: 0, stance: "flying" }
		]);

		expect(timeline.actions.map((action) => action.type)).toEqual(["stance", "say"]);
		expect(timeline.durationMs).toBe(1200);
	});
});
