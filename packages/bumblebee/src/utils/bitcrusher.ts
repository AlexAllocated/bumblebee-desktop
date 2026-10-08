export type BitcrusherParameters = {
	bitDepth?: number;
	sampleRateHz?: number;
	mix?: number;
};

export type BitcrusherOptions = BitcrusherParameters & {
	workletUrl: string;
};

export type BitcrusherNode = {
	node: AudioWorkletNode;
	setMix: (mix: number, transitionSeconds?: number) => void;
	dispose: () => void;
};

const loadedWorklets = new WeakMap<AudioContext, Map<string, Promise<void>>>();

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

export const applyBitcrusherToSamples = (
	samples: Float32Array,
	sourceSampleRate: number,
	options: BitcrusherParameters = {}
) => {
	const bitDepth = clamp(Math.round(options.bitDepth ?? 8), 4, 16);
	const targetSampleRate = clamp(options.sampleRateHz ?? 16_000, 4_000, sourceSampleRate);
	const mix = clamp(options.mix ?? 1, 0, 1);
	const levels = 2 ** (bitDepth - 1);
	const phaseStep = Math.min(1, targetSampleRate / sourceSampleRate);
	const processed = new Float32Array(samples.length);
	let phase = 1;
	let held = 0;

	for (let index = 0; index < samples.length; index += 1) {
		const sample = samples[index] ?? 0;
		phase += phaseStep;
		if (phase >= 1) {
			phase -= 1;
			held = Math.round(sample * levels) / levels;
		}
		processed[index] = sample * (1 - mix) + held * mix;
	}

	return processed;
};

export const createBitcrushedAudioBuffer = (
	context: BaseAudioContext,
	buffer: AudioBuffer,
	options: BitcrusherParameters = {}
) => {
	const processed = context.createBuffer(buffer.numberOfChannels, buffer.length, buffer.sampleRate);
	for (let channel = 0; channel < buffer.numberOfChannels; channel += 1) {
		processed.copyToChannel(
			applyBitcrusherToSamples(buffer.getChannelData(channel), buffer.sampleRate, options),
			channel
		);
	}
	return processed;
};

const loadWorklet = (context: AudioContext, url: string) => {
	let contextModules = loadedWorklets.get(context);
	if (!contextModules) {
		contextModules = new Map();
		loadedWorklets.set(context, contextModules);
	}
	let pending = contextModules.get(url);
	if (!pending) {
		pending = context.audioWorklet.addModule(url).catch((error) => {
			contextModules?.delete(url);
			throw error;
		});
		contextModules.set(url, pending);
	}
	return pending;
};

export const createBitcrusherNode = async (
	context: AudioContext,
	options: BitcrusherOptions
): Promise<BitcrusherNode> => {
	await loadWorklet(context, options.workletUrl);
	const node = new AudioWorkletNode(context, "bumblebee-bitcrusher");
	const bitDepth = node.parameters.get("bitDepth");
	const sampleRateHz = node.parameters.get("sampleRateHz");
	const mix = node.parameters.get("mix");
	bitDepth?.setValueAtTime(clamp(Math.round(options.bitDepth ?? 8), 4, 16), context.currentTime);
	sampleRateHz?.setValueAtTime(
		clamp(options.sampleRateHz ?? 16000, 4000, context.sampleRate),
		context.currentTime
	);
	mix?.setValueAtTime(clamp(options.mix ?? 1, 0, 1), context.currentTime);

	return {
		node,
		setMix(value, transitionSeconds = 0.018) {
			if (!mix) return;
			mix.cancelScheduledValues(context.currentTime);
			mix.setTargetAtTime(
				clamp(value, 0, 1),
				context.currentTime,
				Math.max(0.001, transitionSeconds)
			);
		},
		dispose() {
			node.disconnect();
		}
	};
};
