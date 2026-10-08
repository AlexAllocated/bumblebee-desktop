import { afterEach, describe, expect, spyOn, test } from "bun:test";
import { fetchBumblebeeModelSource, loadBumblebeeModelSource } from "./modelAsset";

const originalFetch = globalThis.fetch;

afterEach(() => {
	globalThis.fetch = originalFetch;
});

describe("Bumblebee model asset loading", () => {
	test("deduplicates concurrent GLB downloads and shares the downloaded bytes", async () => {
		let fetches = 0;
		globalThis.fetch = (async () => {
			fetches += 1;
			return new Response(new Uint8Array([0x67, 0x6c, 0x54, 0x46]));
		}) as unknown as typeof fetch;
		const url = `https://example.com/bumblebee-${crypto.randomUUID()}.glb`;

		const [first, second] = await Promise.all([
			fetchBumblebeeModelSource(url),
			fetchBumblebeeModelSource(url)
		]);

		expect(fetches).toBe(1);
		expect(first).toBe(second);
		expect(Array.from(first)).toEqual([0x67, 0x6c, 0x54, 0x46]);
	});

	test("leaves non-GLB sources on Babylon's URL loading path", async () => {
		let fetches = 0;
		globalThis.fetch = (async () => {
			fetches += 1;
			return new Response("{}");
		}) as unknown as typeof fetch;
		const url = `https://example.com/bumblebee-${crypto.randomUUID()}.gltf`;

		expect(await loadBumblebeeModelSource(url)).toBe(url);
		expect(fetches).toBe(0);
	});
});

test("concurrent consumers recover a transient download failure without duplicating retries", async () => {
	let attempts = 0;
	globalThis.fetch = (async () => {
		attempts += 1;
		if (attempts === 1) throw new TypeError("Load failed");
		return new Response(new Uint8Array([1, 2, 3]));
	}) as unknown as typeof fetch;
	const url = `https://example.com/${crypto.randomUUID()}.glb`;
	const [first, second] = await Promise.all([
		fetchBumblebeeModelSource(url),
		fetchBumblebeeModelSource(url)
	]);
	expect(attempts).toBe(2);
	expect(first).toBe(second);
	expect([...first]).toEqual([1, 2, 3]);
});

test("retries server failure and a truncated body within the same download", async () => {
	let attempts = 0;
	globalThis.fetch = (async () => {
		attempts += 1;
		if (attempts === 1) return new Response(null, { status: 503 });
		if (attempts === 2)
			return new Response(
				new ReadableStream({
					start(controller) {
						controller.error(new TypeError("Load failed"));
					}
				})
			);
		return new Response(new Uint8Array([4]));
	}) as unknown as typeof fetch;
	expect([
		...(await fetchBumblebeeModelSource(`https://example.com/${crypto.randomUUID()}.glb`))
	]).toEqual([4]);
	expect(attempts).toBe(3);
});

test("persistent network failures remain actionable and do not poison the model cache", async () => {
	let attempts = 0;
	globalThis.fetch = (async () => {
		attempts += 1;
		throw new TypeError("Load failed");
	}) as unknown as typeof fetch;
	const url = `https://example.com/${crypto.randomUUID()}.glb`;
	await expect(fetchBumblebeeModelSource(url)).rejects.toMatchObject({
		name: "BumblebeeModelNetworkError"
	});
	expect(attempts).toBe(3);
	globalThis.fetch = (async () => new Response(new Uint8Array([5]))) as unknown as typeof fetch;
	expect([...(await fetchBumblebeeModelSource(url))]).toEqual([5]);
});

test("permanent HTTP failures are surfaced without retry", async () => {
	let attempts = 0;
	globalThis.fetch = (async () => {
		attempts += 1;
		return new Response(null, { status: 404 });
	}) as unknown as typeof fetch;
	await expect(
		fetchBumblebeeModelSource(`https://example.com/${crypto.randomUUID()}.glb`)
	).rejects.toMatchObject({ name: "BumblebeeModelHttpError" });
	expect(attempts).toBe(1);
});

test("the download deadline aborts a stalled body and prevents further retries", async () => {
	const controller = new AbortController();
	let attempts = 0;
	const timeout = spyOn(AbortSignal, "timeout").mockReturnValue(controller.signal);
	globalThis.fetch = (async (_url: Parameters<typeof fetch>[0], options?: RequestInit) => {
		attempts += 1;
		return new Response(
			new ReadableStream({
				start(stream) {
					options?.signal?.addEventListener("abort", () => stream.error(options.signal?.reason), {
						once: true
					});
					queueMicrotask(() => controller.abort(new DOMException("Timed out", "TimeoutError")));
				}
			})
		);
	}) as unknown as typeof fetch;
	try {
		await expect(
			fetchBumblebeeModelSource(`https://example.com/${crypto.randomUUID()}.glb`)
		).rejects.toMatchObject({ name: "BumblebeeModelTimeoutError" });
		expect(timeout).toHaveBeenCalledWith(15000);
		expect(attempts).toBe(1);
	} finally {
		timeout.mockRestore();
	}
});
