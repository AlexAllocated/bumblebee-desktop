import type { BumblebeeEmote } from "../overlay/types";

type BumblebeeEmoteLoopRange = {
	from: number;
	to: number;
};

const loopRanges = {
	// The authored mad clip eases from neutral over frames 0-10, contains two
	// flat-eye blinks that can read as quick smiles, then eases back to neutral
	// over frames 310-320. Loop a single fully-angry frame interval so a sustained
	// mad face neither flashes neutral nor softens during those blinks.
	mad: { from: 10, to: 11 }
} satisfies Partial<Record<BumblebeeEmote, BumblebeeEmoteLoopRange>>;

export const resolveBumblebeeEmoteLoopRange = (
	name: BumblebeeEmote
): BumblebeeEmoteLoopRange | undefined => loopRanges[name as keyof typeof loopRanges];
