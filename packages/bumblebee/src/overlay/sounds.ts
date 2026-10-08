import type { SoundEffect } from "./types";

const soundEffectPaths = {
	bleep: "/audio/bleep.mp3",
	bubblePop: "/audio/bubble-pop.mp3",
	error: ["/audio/error/error01.wav", "/audio/error/error02.wav"],
	message: "/audio/message-sound.mp3",
	perk: "/audio/perk/perk01.wav",
	plink: "/audio/plink.mp3",
	startup: "/audio/startup/startup01.opus",
	techTyping: "/audio/tech-typing.mp3",
	transitionWhoosh: "/audio/caption-morph-whoosh.mp3",
	zoom: "/audio/zoom/zoom01.wav"
} satisfies Record<SoundEffect, string | string[]>;

const soundEffectKeys = Object.keys(soundEffectPaths) as SoundEffect[];

export { soundEffectKeys, soundEffectPaths };
