const timeout = async (ms: number, abort?: AbortSignal) =>
	new Promise((resolve, reject) => {
		if (abort?.aborted) {
			reject(new Error("Aborted"));
			return;
		}
		let timer: ReturnType<typeof setTimeout> | null = null;
		let onAbort: () => void;
		const cleanup = () => {
			if (timer) clearTimeout(timer);
			timer = null;
			abort?.removeEventListener("abort", onAbort);
		};
		onAbort = () => {
			cleanup();
			reject(new Error("Aborted"));
		};
		timer = setTimeout(() => {
			cleanup();
			resolve(undefined);
		}, ms);
		abort?.addEventListener("abort", onAbort, { once: true });
	});

export { timeout };
