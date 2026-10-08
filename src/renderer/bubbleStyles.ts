import type { ChatBubbleStyleTokens } from "@hivetech/bumblebee";
export const chatBubbleStyleSeeds: Array<{ id: string; name: string; description: string; rarity: string; style: ChatBubbleStyleTokens }> = [
	{
		id: "classic-comic",
		name: "Classic Comic",
		description: "Creamy rectangular comic bubble with a clean ink outline.",
		rarity: "common",
		style: {
			background: "#fff7c8",
			border: "#2f2a22",
			text: "#211b16",
			shadow: "rgba(47, 42, 34, 0.25)",
			placeholderColor: "#f2c94c",
			tail: "center",
			fontFamily: "comic",
			shape: "rounded"
		}
	},
	{
		id: "honey-pop",
		name: "Honey Pop",
		description: "Puffy honey-cloud bubble with sticky golden highlights.",
		rarity: "uncommon",
		style: {
			background: "#ffd166",
			border: "#9a5600",
			text: "#3a2400",
			shadow: "rgba(154, 86, 0, 0.34)",
			placeholderColor: "#f59e0b",
			tail: "left",
			fontFamily: "rounded",
			shape: "cloud"
		}
	},
	{
		id: "arcade-caption",
		name: "Arcade Caption",
		description: "Chamfered pixel caption panel for retro game nights.",
		rarity: "rare",
		style: {
			background: "#111827",
			border: "#67e8f9",
			text: "#ecfeff",
			shadow: "rgba(103, 232, 249, 0.38)",
			placeholderColor: "#22d3ee",
			tail: "center",
			fontFamily: "mono",
			shape: "caption"
		}
	},
	{
		id: "starlight-burst",
		name: "Starlight Burst",
		description: "Explosive comic starburst for big reactions.",
		rarity: "epic",
		style: {
			background: "#fff7fb",
			border: "#a855f7",
			text: "#3b0764",
			shadow: "rgba(168, 85, 247, 0.36)",
			placeholderColor: "#a855f7",
			tail: "right",
			fontFamily: "comic",
			shape: "burst"
		}
	},
	{
		id: "forest-whisper",
		name: "Forest Whisper",
		description: "Soft mossy bubble for cozy commentary.",
		rarity: "uncommon",
		style: {
			background: "#d9f99d",
			border: "#3f6212",
			text: "#1a2e05",
			shadow: "rgba(63, 98, 18, 0.26)",
			placeholderColor: "#65a30d",
			tail: "center",
			fontFamily: "serif",
			shape: "rounded"
		}
	},
	{
		id: "neon-terminal",
		name: "Neon Terminal",
		description: "Cut-corner terminal glass with a toxic green edge.",
		rarity: "rare",
		style: {
			background: "rgba(5, 19, 11, 0.94)",
			border: "#39ff88",
			text: "#d9ffe8",
			shadow: "rgba(57, 255, 136, 0.42)",
			placeholderColor: "#22c55e",
			tail: "center",
			fontFamily: "mono",
			shape: "terminal"
		}
	},
	{
		id: "storybook-ribbon",
		name: "Storybook Ribbon",
		description: "Warm storybook paper with ribboned sides.",
		rarity: "epic",
		style: {
			background: "#fff1d6",
			border: "#b45309",
			text: "#3f2507",
			shadow: "rgba(180, 83, 9, 0.28)",
			placeholderColor: "#d97706",
			tail: "left",
			fontFamily: "serif",
			shape: "ribbon"
		}
	},
	{
		id: "moonlit-oval",
		name: "Moonlit Frame",
		description: "Soft rounded bubble for dreamy little asides.",
		rarity: "rare",
		style: {
			background: "#eef2ff",
			border: "#4f46e5",
			text: "#1e1b4b",
			shadow: "rgba(79, 70, 229, 0.28)",
			placeholderColor: "#818cf8",
			tail: "center",
			fontFamily: "rounded",
			shape: "rounded"
		}
	},
	{
		id: "parchment-oval",
		name: "Parchment Panel",
		description: "Warm rectangular parchment panel with storybook margins.",
		rarity: "uncommon",
		style: {
			background: "#fff4d6",
			border: "#92400e",
			text: "#3b2205",
			shadow: "rgba(146, 64, 14, 0.26)",
			placeholderColor: "#b45309",
			tail: "center",
			fontFamily: "serif",
			shape: "rectangle"
		}
	},
	{
		id: "aqua-orbit",
		name: "Aqua Orbit",
		description: "Glassy aqua callout with a bright sci-fi edge.",
		rarity: "rare",
		style: {
			background: "#dffcff",
			border: "#0891b2",
			text: "#083344",
			shadow: "rgba(8, 145, 178, 0.3)",
			placeholderColor: "#06b6d4",
			tail: "center",
			fontFamily: "rounded",
			shape: "caption"
		}
	},
	{
		id: "broadcast-box",
		name: "Broadcast Box",
		description: "Hard-edged studio callout with crisp broadcast energy.",
		rarity: "uncommon",
		style: {
			background: "#f1f5f9",
			border: "#0f172a",
			text: "#0f172a",
			shadow: "rgba(15, 23, 42, 0.24)",
			placeholderColor: "#64748b",
			tail: "right",
			fontFamily: "mono",
			shape: "rectangle"
		}
	},
	{
		id: "newsprint-panel",
		name: "Newsprint Panel",
		description: "Square newsprint panel with Sunday-comics ink.",
		rarity: "rare",
		style: {
			background: "#fffbea",
			border: "#1f2937",
			text: "#111827",
			shadow: "rgba(31, 41, 55, 0.24)",
			placeholderColor: "#475569",
			tail: "center",
			fontFamily: "serif",
			shape: "rectangle"
		}
	},
	{
		id: "glitch-card",
		name: "Glitch Card",
		description: "Square digital card with a bright arcade edge.",
		rarity: "rare",
		style: {
			background: "#09090b",
			border: "#f472b6",
			text: "#fdf2f8",
			shadow: "rgba(244, 114, 182, 0.34)",
			placeholderColor: "#f472b6",
			tail: "right",
			fontFamily: "mono",
			shape: "rectangle"
		}
	},
	{
		id: "cosmic-sparkle",
		name: "Cosmic Sparkle",
		description: "Starry midnight bubble with tiny twinkling specks.",
		rarity: "epic",
		style: {
			background: "#1e1b4b",
			border: "#c4b5fd",
			text: "#f5f3ff",
			shadow: "rgba(196, 181, 253, 0.36)",
			placeholderColor: "#a78bfa",
			tail: "center",
			fontFamily: "rounded",
			shape: "rounded",
			decoration: "sparkles",
			motion: "twinkle"
		}
	},
	{
		id: "storm-callout",
		name: "Storm Callout",
		description: "Rain-streaked cloud bubble for dramatic weather reports.",
		rarity: "rare",
		style: {
			background: "#dbeafe",
			border: "#1d4ed8",
			text: "#172554",
			shadow: "rgba(29, 78, 216, 0.28)",
			placeholderColor: "#3b82f6",
			tail: "left",
			fontFamily: "comic",
			shape: "cloud",
			decoration: "rain",
			motion: "drift"
		}
	},
	{
		id: "party-popper",
		name: "Party Popper",
		description: "Confetti-packed comic bubble for maximum celebration.",
		rarity: "epic",
		style: {
			background: "#fff7ed",
			border: "#ea580c",
			text: "#431407",
			shadow: "rgba(234, 88, 12, 0.3)",
			placeholderColor: "#fb923c",
			tail: "right",
			fontFamily: "rounded",
			shape: "burst",
			decoration: "confetti",
			motion: "sprinkle"
		}
	},
	{
		id: "lava-lamp",
		name: "Lava Lamp",
		description: "Groovy molten blobs drifting through a rounded bubble.",
		rarity: "rare",
		style: {
			background: "#2e1065",
			border: "#fb7185",
			text: "#fff1f2",
			shadow: "rgba(251, 113, 133, 0.36)",
			placeholderColor: "#fb7185",
			tail: "center",
			fontFamily: "rounded",
			shape: "rounded",
			decoration: "lava",
			motion: "ooze"
		}
	},
	{
		id: "manga-pop",
		name: "Manga Pop",
		description: "Halftone ink panel with Sunday-comic swagger.",
		rarity: "rare",
		style: {
			background: "#fff1f2",
			border: "#be123c",
			text: "#4c0519",
			shadow: "rgba(190, 18, 60, 0.28)",
			placeholderColor: "#e11d48",
			tail: "left",
			fontFamily: "comic",
			shape: "caption",
			decoration: "halftone",
			motion: "pulse"
		}
	},
	{
		id: "soundwave",
		name: "Soundwave",
		description: "Audio-reactive terminal bubble with scanning waveform bars.",
		rarity: "epic",
		style: {
			background: "rgba(17, 24, 39, 0.95)",
			border: "#facc15",
			text: "#fefce8",
			shadow: "rgba(250, 204, 21, 0.36)",
			placeholderColor: "#eab308",
			tail: "center",
			fontFamily: "mono",
			shape: "terminal",
			decoration: "waveform",
			motion: "scan"
		}
	}
];
