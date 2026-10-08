import { createRuntimeHost } from "../runtimeHost";
import type { OverlayOptions } from "../overlay/types";

type BumblebeeModelPreloadOptions = Pick<OverlayOptions, "assets" | "assetBaseUrl">;

const modelSourcePromises = new Map<string, Promise<Uint8Array>>();

const isGlbModelUrl = (url: string) => {
	try {
		return new URL(url, globalThis.location?.href ?? "http://localhost").pathname.endsWith(".glb");
	} catch {
		return false;
	}
};

const modelDownloadError = (name: string, cause: unknown) => {
	const error = new Error("Bumblebee model download failed.", { cause });
	error.name = name;
	return error;
};

const downloadBumblebeeModel = async (url: string) => {
	// One budget covers headers, body reads and all retries. Only this idempotent
	// asset download is retried; all consumers share the same in-flight promise.
	const signal = AbortSignal.timeout(15_000);
	let failure: Error | undefined;
	for (let attempt = 0; attempt < 3; attempt += 1) {
		try {
			const response = await fetch(url, { signal });
			if (response.ok) return new Uint8Array(await response.arrayBuffer());
			await response.body?.cancel();
			failure = modelDownloadError("BumblebeeModelHttpError", new Error(`HTTP ${response.status}`));
			if (response.status < 500) throw failure;
		} catch (error) {
			if (signal.aborted) throw modelDownloadError("BumblebeeModelTimeoutError", error);
			if (!(error instanceof TypeError)) throw error;
			failure = modelDownloadError("BumblebeeModelNetworkError", error);
		}
		if (attempt < 2) await new Promise((resolve) => setTimeout(resolve, 250 * (attempt + 1)));
	}
	throw failure;
};

const fetchBumblebeeModelSource = (url: string) => {
	const existing = modelSourcePromises.get(url);
	if (existing) return existing;
	const pending = downloadBumblebeeModel(url).catch((error) => {
		modelSourcePromises.delete(url);
		throw error;
	});
	modelSourcePromises.set(url, pending);
	return pending;
};

const loadBumblebeeModelSource = (url: string) =>
	isGlbModelUrl(url) ? fetchBumblebeeModelSource(url) : Promise.resolve(url);

const resolveBumblebeeModelUrl = (options?: BumblebeeModelPreloadOptions) => {
	const host = createRuntimeHost(options);
	return (
		options?.assets?.bumblebeeModelUrl?.toString() ?? host.httpUrl("models/bumblebee.cb67e11b.glb")
	);
};

const preloadBumblebeeModel = async (options?: BumblebeeModelPreloadOptions) => {
	const url = resolveBumblebeeModelUrl(options);
	if (!isGlbModelUrl(url)) return;
	await fetchBumblebeeModelSource(url);
};

export {
	fetchBumblebeeModelSource,
	loadBumblebeeModelSource,
	preloadBumblebeeModel,
	resolveBumblebeeModelUrl
};
export type { BumblebeeModelPreloadOptions };
