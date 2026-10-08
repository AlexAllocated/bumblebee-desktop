// Tiny logging helper for overlay/puppet debugging.
// Enable/disable globally via: window.__BB_LOG = true|false
// Keep logs short, single-line, with small objects.

declare global {
	interface Window {
		__BB_LOG?: boolean;
	}
}

export function bbLog(module: string, event: string, data?: unknown) {
	try {
		const enabled = typeof window !== "undefined" ? (window.__BB_LOG ?? false) : false;
		if (!enabled) return;
	} catch {}
	let tail = "";
	if (data !== undefined) {
		try {
			tail = " " + JSON.stringify(data);
		} catch {
			tail = " [unserializable]";
		}
	}
	console.log(`[BB][${module}] ${event}${tail}`);
}

export function bbNow() {
	return Date.now();
}
