import { abortable } from "../utils/abortable";
import { createActor } from "xstate";
import { audioMachine } from "./machines";
import { calculateByteRmsLevel, smoothAudioLevel } from "../utils/audioLevels";
import type { SoundEffectPlaybackOptions, Speech, VoiceChirpKind } from "./types";
import {
	createBitcrushedAudioBuffer,
	createBitcrusherNode,
	type BitcrusherNode
} from "../utils/bitcrusher";
import type { DialogueBleepCue } from "./dialogueBleeps";
import type { TimedSpeechAudioCue } from "./speechAudioCues";

export const createReversedAudioBuffer = (context: BaseAudioContext, buffer: AudioBuffer) => {
	const reversed = context.createBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
	for (let channel = 0; channel < buffer.numberOfChannels; channel += 1) {
		const source = buffer.getChannelData(channel);
		const target = reversed.getChannelData(channel);
		for (let index = 0; index < source.length; index += 1) {
			target[index] = source[source.length - index - 1];
		}
	}
	return reversed;
};

export const CALL_WAITING_CHIME = {
	level: 0.26,
	notes: [
		{ frequencyHz: 523, durationMs: 70, fadeInMs: 6, fadeOutMs: 18, delayMs: 0 },
		{ frequencyHz: 659, durationMs: 85, fadeInMs: 6, fadeOutMs: 18, delayMs: 115 }
	]
} as const;

export const VOICE_CHIRPS = {
	wake_chirp: {
		level: 0.26,
		notes: [
			{ startHz: 620, endHz: 980, durationMs: 70, fadeInMs: 4, fadeOutMs: 12, delayMs: 0 },
			{ startHz: 700, endHz: 1160, durationMs: 85, fadeInMs: 4, fadeOutMs: 12, delayMs: 98 }
		]
	},
	heard_chirp: {
		level: 0.22,
		notes: [
			{ startHz: 1120, endHz: 760, durationMs: 55, fadeInMs: 5, fadeOutMs: 16, delayMs: 0 },
			{ startHz: 960, endHz: 620, durationMs: 72, fadeInMs: 5, fadeOutMs: 16, delayMs: 79 }
		]
	}
} as const;

export const THINKING_SOUND_LOOP_MS = 1000;
export const THINKING_SOUND_LEVEL = 0.1;
export const THINKING_SOUND_VARIANTS = [
	[
		{ start: 0.05, frequencyHz: 480, duration: 0.07, amplitude: 0.55, glide: 0.05 },
		{ start: 0.18, frequencyHz: 520, duration: 0.06, amplitude: 0.45, glide: 0.04 },
		{ start: 0.32, frequencyHz: 440, duration: 0.08, amplitude: 0.5, glide: 0.05 },
		{ start: 0.46, frequencyHz: 550, duration: 0.06, amplitude: 0.42, glide: 0.04 },
		{ start: 0.62, frequencyHz: 470, duration: 0.07, amplitude: 0.5, glide: 0.05 },
		{ start: 0.78, frequencyHz: 580, duration: 0.06, amplitude: 0.4, glide: 0.04 }
	],
	[
		{ start: 0.07, frequencyHz: 500, duration: 0.07, amplitude: 0.5, glide: 0.05 },
		{ start: 0.2, frequencyHz: 550, duration: 0.06, amplitude: 0.45, glide: 0.04 },
		{ start: 0.34, frequencyHz: 450, duration: 0.08, amplitude: 0.52, glide: 0.06 },
		{ start: 0.5, frequencyHz: 530, duration: 0.06, amplitude: 0.42, glide: 0.04 },
		{ start: 0.66, frequencyHz: 490, duration: 0.07, amplitude: 0.48, glide: 0.05 },
		{ start: 0.82, frequencyHz: 580, duration: 0.06, amplitude: 0.4, glide: 0.04 }
	],
	[
		{ start: 0.04, frequencyHz: 520, duration: 0.07, amplitude: 0.5, glide: 0.05 },
		{ start: 0.16, frequencyHz: 580, duration: 0.06, amplitude: 0.44, glide: 0.04 },
		{ start: 0.3, frequencyHz: 470, duration: 0.08, amplitude: 0.55, glide: 0.06 },
		{ start: 0.44, frequencyHz: 560, duration: 0.06, amplitude: 0.42, glide: 0.04 },
		{ start: 0.6, frequencyHz: 500, duration: 0.07, amplitude: 0.5, glide: 0.05 },
		{ start: 0.76, frequencyHz: 600, duration: 0.06, amplitude: 0.4, glide: 0.04 }
	]
] as const;

type SpeechAudioFilter = NonNullable<Speech["audioFilter"]>;

const finiteOr = (value: number | undefined, fallback: number) =>
	typeof value === "number" && Number.isFinite(value) ? value : fallback;

export const resolveSpeechAudioFilter = (filter: SpeechAudioFilter, sampleRate: number) => {
	const nyquist = Math.max(40, sampleRate / 2);
	const clampFrequency = (value: number | undefined, fallback: number) =>
		Math.max(20, Math.min(nyquist - 1, finiteOr(value, fallback)));
	return {
		highpassHz: clampFrequency(filter.highpassHz, 20),
		lowpassHz: clampFrequency(filter.lowpassHz, nyquist - 1),
		presence: filter.presence
			? {
					frequencyHz: clampFrequency(filter.presence.frequencyHz, 1_200),
					gainDb: Math.max(-24, Math.min(24, finiteOr(filter.presence.gainDb, 0))),
					q: Math.max(0.0001, Math.min(30, finiteOr(filter.presence.q, 1)))
				}
			: null,
		compressor: filter.compressor
			? {
					thresholdDb: Math.max(-100, Math.min(0, finiteOr(filter.compressor.thresholdDb, -24))),
					kneeDb: Math.max(0, Math.min(40, finiteOr(filter.compressor.kneeDb, 20))),
					ratio: Math.max(1, Math.min(20, finiteOr(filter.compressor.ratio, 4))),
					attackSeconds: Math.max(0, Math.min(1, finiteOr(filter.compressor.attackSeconds, 0.01))),
					releaseSeconds: Math.max(0, Math.min(1, finiteOr(filter.compressor.releaseSeconds, 0.2)))
				}
			: null
	};
};

export class AudioPlaybackUnavailableError extends Error {
	constructor(message = "Audio playback is not available.") {
		super(message);
		this.name = "AudioPlaybackUnavailableError";
	}
}

export class Audio {
	readonly actorRef = createActor(audioMachine).start();
	readonly context: AudioContext;
	readonly output: GainNode;
	readonly analyser: AnalyserNode;
	readonly #effectBuffers = new Map<string, AudioBuffer[]>();
	readonly #effectLoadPromises = new Map<string, Promise<void>>();
	readonly #reversedEffectBuffers = new Map<string, AudioBuffer[]>();
	#resumeOnUserGesture: (() => void) | null = null;
	#automaticResumeEnabled = true;
	#handleContextStateChange: (() => void) | null = null;
	#handleVisibilityChange: (() => void) | null = null;

	constructor() {
		this.actorRef.send({ type: "CREATE" });
		this.context = new AudioContext();
		this.output = this.context.createGain();
		this.analyser = this.context.createAnalyser();
		this.analyser.connect(this.output);
		this.output.connect(this.context.destination);
		this.actorRef.send({ type: this.context.state === "running" ? "RUNNING" : "SUSPENDED" });

		this.#handleContextStateChange = () => {
			const state = this.#state();
			this.actorRef.send({ type: state === "running" ? "RUNNING" : "SUSPENDED" });
			if (state === "running") {
				this.#removeResumeListeners();
			} else if (state !== "closed") {
				this.#installResumeListeners();
			}
		};
		this.context.addEventListener("statechange", this.#handleContextStateChange);

		this.#handleVisibilityChange = () => {
			if (
				!this.#automaticResumeEnabled ||
				document.visibilityState !== "visible" ||
				this.#state() === "closed"
			) {
				return;
			}
			this.#installResumeListeners();
			if (navigator.userActivation?.hasBeenActive) {
				void this.resume().catch(() => undefined);
			}
		};
		document.addEventListener("visibilitychange", this.#handleVisibilityChange);
		this.#installResumeListeners();
	}

	#state(): AudioContextState {
		return this.context.state;
	}

	#installResumeListeners() {
		if (!this.#automaticResumeEnabled || this.#resumeOnUserGesture || this.#state() === "closed")
			return;
		const resume = () => {
			if (!this.#automaticResumeEnabled) return;
			void this.resume()
				.then(() => {
					if (this.#state() === "running") this.#removeResumeListeners();
				})
				.catch(() => undefined);
		};
		this.#resumeOnUserGesture = resume;
		window.addEventListener("pointerdown", resume, { capture: true });
		window.addEventListener("touchend", resume, { capture: true });
		window.addEventListener("keydown", resume, { capture: true });
	}

	#removeResumeListeners() {
		const resume = this.#resumeOnUserGesture;
		if (!resume) return;
		window.removeEventListener("pointerdown", resume, { capture: true });
		window.removeEventListener("touchend", resume, { capture: true });
		window.removeEventListener("keydown", resume, { capture: true });
		this.#resumeOnUserGesture = null;
	}

	setAutomaticResumeEnabled(enabled: boolean) {
		this.#automaticResumeEnabled = enabled;
		if (!enabled) {
			this.#removeResumeListeners();
			return;
		}
		if (this.#state() !== "running" && this.#state() !== "closed") {
			this.#installResumeListeners();
		}
	}

	async resume() {
		if (this.#state() === "closed") return;
		if (this.#state() === "running") return;
		this.actorRef.send({ type: "UNLOCK" });
		const resumeAttempt = this.context.resume();
		const resumed = await Promise.race([
			resumeAttempt.then(
				() => true,
				() => false
			),
			new Promise<false>((resolve) => setTimeout(() => resolve(false), 1500))
		]);
		if (!resumed && this.#state() !== "running") {
			this.actorRef.send({ type: this.#state() === "running" ? "RUNNING" : "SUSPENDED" });
			return;
		}
		this.actorRef.send({ type: this.#state() === "running" ? "RUNNING" : "SUSPENDED" });
	}

	canAttemptPlayback() {
		if (this.#state() === "running") return true;
		const userActivation = navigator.userActivation;
		return !userActivation || userActivation.hasBeenActive || userActivation.isActive;
	}

	async #ensurePlaybackReady() {
		if (this.#state() === "running") return true;
		if (!this.canAttemptPlayback()) return false;
		try {
			await this.resume();
		} catch {
			return false;
		}
		return this.#state() === "running";
	}

	async play(
		chunk: Uint8Array,
		options?: {
			signal?: AbortSignal;
			analyze?: boolean;
			onReady?: (durationMs: number) => void | Promise<void>;
			onStart?: (durationMs: number) => void;
			onLevel?: (level: number, nowMs: number) => void;
			onTime?: (elapsedMs: number) => void;
			bitcrusher?: {
				bitDepth?: number;
				sampleRateHz?: number;
				mix?: number;
				workletUrl: string;
				enabled?: boolean | (() => boolean);
			};
			filter?: SpeechAudioFilter;
			bleeps?: DialogueBleepCue[];
			cues?: TimedSpeechAudioCue[];
		}
	) {
		if (!(await abortable(this.#ensurePlaybackReady(), options?.signal))) {
			throw new AudioPlaybackUnavailableError(
				"Audio playback skipped because audio is not unlocked."
			);
		}
		const copy = chunk.slice();
		const buffer = await abortable(this.context.decodeAudioData(copy.buffer), options?.signal);
		if (options?.signal?.aborted) return false;
		const durationMs = buffer.duration * 1000;
		await options?.onReady?.(durationMs);
		if (options?.signal?.aborted) return false;
		const analyze = options?.analyze !== false;
		const source = this.context.createBufferSource();
		const speechGain = this.context.createGain();
		source.buffer = buffer;
		const filterNodes: AudioNode[] = [];
		let speechOutput: AudioNode = speechGain;
		if (options?.filter) {
			const filter = resolveSpeechAudioFilter(options.filter, this.context.sampleRate);
			const highpass = this.context.createBiquadFilter();
			highpass.type = "highpass";
			highpass.frequency.value = filter.highpassHz;
			speechOutput.connect(highpass);
			filterNodes.push(highpass);
			speechOutput = highpass;

			const lowpass = this.context.createBiquadFilter();
			lowpass.type = "lowpass";
			lowpass.frequency.value = filter.lowpassHz;
			speechOutput.connect(lowpass);
			filterNodes.push(lowpass);
			speechOutput = lowpass;

			if (filter.presence) {
				const presence = this.context.createBiquadFilter();
				presence.type = "peaking";
				presence.frequency.value = filter.presence.frequencyHz;
				presence.gain.value = filter.presence.gainDb;
				presence.Q.value = filter.presence.q;
				speechOutput.connect(presence);
				filterNodes.push(presence);
				speechOutput = presence;
			}

			if (filter.compressor) {
				const compressor = this.context.createDynamicsCompressor();
				compressor.threshold.value = filter.compressor.thresholdDb;
				compressor.knee.value = filter.compressor.kneeDb;
				compressor.ratio.value = filter.compressor.ratio;
				compressor.attack.value = filter.compressor.attackSeconds;
				compressor.release.value = filter.compressor.releaseSeconds;
				speechOutput.connect(compressor);
				filterNodes.push(compressor);
				speechOutput = compressor;
			}
		}
		let bitcrusher: BitcrusherNode | null = null;
		if (options?.bitcrusher) {
			try {
				const wetMix = Math.max(0, Math.min(1, options.bitcrusher.mix ?? 1));
				const enabled =
					typeof options.bitcrusher.enabled === "function"
						? options.bitcrusher.enabled()
						: (options.bitcrusher.enabled ?? true);
				bitcrusher = await createBitcrusherNode(this.context, {
					...options.bitcrusher,
					mix: enabled ? wetMix : 0
				});
			} catch (error) {
				console.warn("Speech bitcrusher unavailable; continuing with clean audio.", error);
			}
		}
		source.connect(speechGain);
		if (bitcrusher) {
			speechOutput.connect(bitcrusher.node);
			bitcrusher.node.connect(analyze ? this.analyser : this.output);
		} else {
			speechOutput.connect(analyze ? this.analyser : this.output);
		}
		const bleepNodes: Array<{
			oscillator: OscillatorNode;
			filter: BiquadFilterNode;
			gain: GainNode;
		}> = [];
		// Leave a small scheduling runway so a cue at the first word cannot land in
		// the past while its Web Audio nodes are being connected.
		const playbackStart = this.context.currentTime + 0.01;
		const playbackEnd = playbackStart + buffer.duration;
		const cueOscillators: OscillatorNode[] = [];
		for (const cue of options?.cues ?? []) {
			const cueStart = playbackStart + cue.offsetMs / 1_000;
			if (cueStart >= playbackEnd) continue;
			if (cue.kind === "call_waiting") {
				cueOscillators.push(...this.#scheduleCallWaitingChime(cueStart, cue.volume).oscillators);
			} else {
				cueOscillators.push(
					...this.#scheduleVoiceChirp(cue.kind, cueStart, cue.volume).oscillators
				);
			}
		}
		speechGain.gain.setValueAtTime(1, playbackStart);
		for (const cue of options?.bleeps ?? []) {
			const cueStart = Math.max(playbackStart, playbackStart + cue.offsetMs / 1_000);
			const cueEnd = Math.min(playbackEnd, cueStart + cue.durationMs / 1_000);
			if (cueEnd <= cueStart) continue;
			const fadeDuration = Math.min(0.008, (cueEnd - cueStart) / 4);
			speechGain.gain.setValueAtTime(1, cueStart);
			speechGain.gain.linearRampToValueAtTime(0, cueStart + fadeDuration);
			speechGain.gain.setValueAtTime(0, Math.max(cueStart + fadeDuration, cueEnd - 0.012));
			speechGain.gain.linearRampToValueAtTime(1, cueEnd);

			const oscillator = this.context.createOscillator();
			const filter = this.context.createBiquadFilter();
			const gain = this.context.createGain();
			oscillator.type = "square";
			oscillator.frequency.setValueAtTime(980, cueStart);
			filter.type = "lowpass";
			filter.frequency.setValueAtTime(2_100, cueStart);
			filter.Q.setValueAtTime(0.8, cueStart);
			gain.gain.setValueAtTime(0.0001, cueStart);
			gain.gain.exponentialRampToValueAtTime(0.055, cueStart + fadeDuration);
			gain.gain.setValueAtTime(0.055, Math.max(cueStart + fadeDuration, cueEnd - 0.012));
			gain.gain.exponentialRampToValueAtTime(0.0001, cueEnd);
			oscillator
				.connect(filter)
				.connect(gain)
				.connect(bitcrusher?.node ?? (analyze ? this.analyser : this.output));
			oscillator.start(cueStart);
			oscillator.stop(cueEnd);
			const nodes = { oscillator, filter, gain };
			bleepNodes.push(nodes);
			oscillator.addEventListener(
				"ended",
				() => {
					oscillator.disconnect();
					filter.disconnect();
					gain.disconnect();
					const index = bleepNodes.indexOf(nodes);
					if (index >= 0) bleepNodes.splice(index, 1);
				},
				{ once: true }
			);
		}
		let animationFrameId: number | null = null;
		let levelEma = 0;
		let lastLevelSampleMs = 0;
		const stopLevelSampler = () => {
			if (animationFrameId === null) return;
			cancelAnimationFrame(animationFrameId);
			animationFrameId = null;
		};
		const startPlaybackSampler = () => {
			const tracksLevel = analyze && Boolean(options?.onLevel);
			const tracksBitcrusher =
				Boolean(bitcrusher) && typeof options?.bitcrusher?.enabled === "function";
			if (!tracksLevel && !tracksBitcrusher && !options?.onTime) return;
			this.analyser.fftSize = 2048;
			const data = new Uint8Array(this.analyser.frequencyBinCount);
			const sample = (nowMs: number) => {
				animationFrameId = requestAnimationFrame(sample);
				options?.onTime?.(Math.max(0, (this.context.currentTime - playbackStart) * 1000));
				if (nowMs - lastLevelSampleMs < 50) return;
				lastLevelSampleMs = nowMs;
				if (tracksLevel) {
					this.analyser.getByteTimeDomainData(data);
					levelEma = smoothAudioLevel(levelEma, calculateByteRmsLevel(data));
					options?.onLevel?.(levelEma, nowMs);
				}
				if (tracksBitcrusher && bitcrusher) {
					const enabled = options?.bitcrusher?.enabled;
					const wetMix = Math.max(0, Math.min(1, options?.bitcrusher?.mix ?? 1));
					bitcrusher.setMix(typeof enabled === "function" && enabled() ? wetMix : 0);
				}
			};
			animationFrameId = requestAnimationFrame(sample);
		};
		const stop = () => {
			try {
				source.stop();
			} catch {
				// Already stopped.
			}
			for (const { oscillator } of bleepNodes) {
				try {
					oscillator.stop();
				} catch {
					// Already stopped.
				}
			}
			for (const oscillator of cueOscillators) {
				try {
					oscillator.stop();
				} catch {
					// Already stopped.
				}
			}
		};
		options?.signal?.addEventListener("abort", stop, { once: true });
		source.start(playbackStart);
		startPlaybackSampler();
		options?.onStart?.(durationMs);
		await new Promise<void>((resolve) => {
			source.onended = () => resolve();
		});
		stopLevelSampler();
		options?.signal?.removeEventListener("abort", stop);
		source.disconnect();
		speechGain.disconnect();
		for (const node of filterNodes) node.disconnect();
		bitcrusher?.dispose();
		return !options?.signal?.aborted;
	}

	setVolume(value: number) {
		const clamped = Math.max(0, Math.min(2, Number.isFinite(value) ? value : 1));
		this.output.gain.setTargetAtTime(clamped, this.context.currentTime, 0.015);
	}

	async preloadEffect(cacheKey: string, urls: string[]) {
		if (this.#effectBuffers.has(cacheKey)) return;
		const pending = this.#effectLoadPromises.get(cacheKey);
		if (pending) return pending;
		const load = Promise.all(
			urls.map(async (url) => {
				const response = await fetch(url);
				if (!response.ok) throw new Error(`Failed to load sound effect: ${url}`);
				const arrayBuffer = await response.arrayBuffer();
				return this.context.decodeAudioData(arrayBuffer);
			})
		)
			.then((buffers) => {
				this.#effectBuffers.set(cacheKey, buffers);
			})
			.finally(() => {
				this.#effectLoadPromises.delete(cacheKey);
			});
		this.#effectLoadPromises.set(cacheKey, load);
		return load;
	}

	async playEffect(
		cacheKey: string,
		urls: string[],
		volume = 1,
		options?: SoundEffectPlaybackOptions
	) {
		if (!this.#effectBuffers.has(cacheKey)) {
			await this.preloadEffect(cacheKey, urls);
		}
		if (!(await this.#ensurePlaybackReady())) {
			throw new Error(`Sound effect ${cacheKey} skipped because audio is not unlocked.`);
		}
		const forwardBuffers = this.#effectBuffers.get(cacheKey) ?? [];
		let buffers = forwardBuffers;
		if (options?.reverse) {
			buffers = this.#reversedEffectBuffers.get(cacheKey) ?? [];
			if (!buffers.length) {
				buffers = forwardBuffers.map((buffer) => createReversedAudioBuffer(this.context, buffer));
				this.#reversedEffectBuffers.set(cacheKey, buffers);
			}
		}
		const buffer = buffers[Math.floor(Math.random() * buffers.length)];
		if (!buffer) throw new Error(`Sound effect ${cacheKey} has no decoded buffers.`);
		const source = this.context.createBufferSource();
		const gain = this.context.createGain();
		source.buffer = options?.bitcrusher
			? createBitcrushedAudioBuffer(this.context, buffer, options.bitcrusher)
			: buffer;
		source.playbackRate.value = Math.max(
			0.01,
			Number.isFinite(options?.playbackRate) ? (options?.playbackRate ?? 1) : 1
		);
		gain.gain.value = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 1));
		source.connect(gain);
		gain.connect(this.output);
		source.addEventListener(
			"ended",
			() => {
				source.disconnect();
				gain.disconnect();
			},
			{ once: true }
		);
		source.start();
		return source;
	}

	async playCallWaitingChime(volume = 1) {
		if (!(await this.#ensurePlaybackReady())) {
			throw new Error("Call-waiting chime skipped because audio is not unlocked.");
		}
		const startTime = this.context.currentTime + 0.005;
		await this.#scheduleCallWaitingChime(startTime, volume).done;
	}

	async playVoiceChirp(kind: VoiceChirpKind, volume = 1) {
		if (!(await this.#ensurePlaybackReady())) {
			throw new Error("Voice chirp skipped because audio is not unlocked.");
		}
		const startTime = this.context.currentTime + 0.005;
		await this.#scheduleVoiceChirp(kind, startTime, volume).done;
	}

	async playThinkingSound(durationMs = THINKING_SOUND_LOOP_MS, volume = 1) {
		if (!(await this.#ensurePlaybackReady())) {
			throw new Error("Thinking sound skipped because audio is not unlocked.");
		}
		const normalizedDurationMs = Math.max(1, Math.round(durationMs));
		const clampedVolume = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 1));
		const startTime = this.context.currentTime + 0.005;
		const endTime = startTime + normalizedDurationMs / 1000;
		const oscillators: OscillatorNode[] = [];
		const loopCount = Math.ceil(normalizedDurationMs / THINKING_SOUND_LOOP_MS);
		for (let loopIndex = 0; loopIndex < loopCount; loopIndex += 1) {
			const variant =
				THINKING_SOUND_VARIANTS[
					Math.min(
						THINKING_SOUND_VARIANTS.length - 1,
						Math.floor(Math.random() * THINKING_SOUND_VARIANTS.length)
					)
				]!;
			const loopStart = startTime + (loopIndex * THINKING_SOUND_LOOP_MS) / 1000;
			for (const event of variant) {
				const eventStart = loopStart + event.start;
				const eventEnd = Math.min(endTime, eventStart + event.duration);
				if (eventEnd <= eventStart) continue;
				const oscillator = this.context.createOscillator();
				const gain = this.context.createGain();
				const peak = Math.max(0.0001, THINKING_SOUND_LEVEL * event.amplitude * clampedVolume);
				oscillator.type = "sine";
				oscillator.frequency.setValueAtTime(event.frequencyHz, eventStart);
				oscillator.frequency.linearRampToValueAtTime(
					event.frequencyHz * (1 + event.glide),
					eventEnd
				);
				gain.gain.setValueAtTime(peak, eventStart);
				gain.gain.exponentialRampToValueAtTime(0.0001, eventEnd);
				oscillator.connect(gain);
				gain.connect(this.output);
				oscillator.addEventListener(
					"ended",
					() => {
						oscillator.disconnect();
						gain.disconnect();
					},
					{ once: true }
				);
				oscillator.start(eventStart);
				oscillator.stop(eventEnd);
				oscillators.push(oscillator);
			}
		}
		const durationClock = this.context.createOscillator();
		const durationGain = this.context.createGain();
		durationGain.gain.setValueAtTime(0, startTime);
		durationClock.connect(durationGain);
		durationGain.connect(this.output);
		await new Promise<void>((resolve) => {
			durationClock.addEventListener(
				"ended",
				() => {
					durationClock.disconnect();
					durationGain.disconnect();
					resolve();
				},
				{ once: true }
			);
			durationClock.start(startTime);
			durationClock.stop(endTime);
		});
		return oscillators;
	}

	#scheduleVoiceChirp(kind: VoiceChirpKind, startTime: number, volume = 1) {
		const chirp = VOICE_CHIRPS[kind];
		const clampedVolume = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 1));
		const oscillators: OscillatorNode[] = [];
		const done = Promise.all(
			chirp.notes.map(
				(note) =>
					new Promise<void>((resolve) => {
						const oscillator = this.context.createOscillator();
						const gain = this.context.createGain();
						const noteStart = startTime + note.delayMs / 1000;
						const noteEnd = noteStart + note.durationMs / 1000;
						const peak = chirp.level * clampedVolume;
						oscillator.type = "sine";
						oscillators.push(oscillator);
						oscillator.frequency.setValueAtTime(note.startHz, noteStart);
						oscillator.frequency.linearRampToValueAtTime(note.endHz, noteEnd);
						gain.gain.setValueAtTime(0, noteStart);
						gain.gain.linearRampToValueAtTime(peak, noteStart + note.fadeInMs / 1000);
						gain.gain.setValueAtTime(peak, noteEnd - note.fadeOutMs / 1000);
						gain.gain.linearRampToValueAtTime(0, noteEnd);
						oscillator.connect(gain);
						gain.connect(this.output);
						oscillator.addEventListener(
							"ended",
							() => {
								oscillator.disconnect();
								gain.disconnect();
								resolve();
							},
							{ once: true }
						);
						oscillator.start(noteStart);
						oscillator.stop(noteEnd);
					})
			)
		);
		return { oscillators, done: done.then(() => undefined) };
	}

	#scheduleCallWaitingChime(startTime: number, volume = 1) {
		const clampedVolume = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 1));
		const oscillators: OscillatorNode[] = [];
		const done = Promise.all(
			CALL_WAITING_CHIME.notes.map(
				(note) =>
					new Promise<void>((resolve) => {
						const oscillator = this.context.createOscillator();
						const gain = this.context.createGain();
						const noteStart = startTime + note.delayMs / 1000;
						const noteEnd = noteStart + note.durationMs / 1000;
						const peak = CALL_WAITING_CHIME.level * clampedVolume;
						oscillator.type = "sine";
						oscillators.push(oscillator);
						oscillator.frequency.setValueAtTime(note.frequencyHz, noteStart);
						gain.gain.setValueAtTime(0, noteStart);
						gain.gain.linearRampToValueAtTime(peak, noteStart + note.fadeInMs / 1000);
						gain.gain.setValueAtTime(peak, noteEnd - note.fadeOutMs / 1000);
						gain.gain.linearRampToValueAtTime(0, noteEnd);
						oscillator.connect(gain);
						gain.connect(this.output);
						oscillator.addEventListener(
							"ended",
							() => {
								oscillator.disconnect();
								gain.disconnect();
								resolve();
							},
							{ once: true }
						);
						oscillator.start(noteStart);
						oscillator.stop(noteEnd);
					})
			)
		);
		return { oscillators, done: done.then(() => undefined) };
	}

	clearEffectCache() {
		this.#effectBuffers.clear();
		this.#effectLoadPromises.clear();
		this.#reversedEffectBuffers.clear();
	}

	async dispose() {
		this.actorRef.send({ type: "DISPOSE" });
		this.#removeResumeListeners();
		if (this.#handleContextStateChange) {
			this.context.removeEventListener("statechange", this.#handleContextStateChange);
			this.#handleContextStateChange = null;
		}
		if (this.#handleVisibilityChange) {
			document.removeEventListener("visibilitychange", this.#handleVisibilityChange);
			this.#handleVisibilityChange = null;
		}
		this.clearEffectCache();
		if (this.#state() !== "closed") {
			await this.context.close();
		}
		this.actorRef.stop();
	}
}
