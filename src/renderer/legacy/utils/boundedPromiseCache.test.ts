import { describe, expect, test } from "bun:test";
import { BoundedPromiseCache } from "./boundedPromiseCache";

describe("BoundedPromiseCache", () => {
	test("evicts the least recently used resolved entry", async () => {
		const evicted: number[] = [];
		const cache = new BoundedPromiseCache<number>(2, (value) => evicted.push(value));

		cache.set("first", Promise.resolve(1));
		cache.set("second", Promise.resolve(2));
		expect(await cache.get("first")).toBe(1);

		cache.set("third", Promise.resolve(3));
		await Promise.resolve();

		expect(cache.get("second")).toBeUndefined();
		expect(await cache.get("first")).toBe(1);
		expect(await cache.get("third")).toBe(3);
		expect(evicted).toEqual([2]);
	});

	test("removes failed loads so a later request can retry", async () => {
		const cache = new BoundedPromiseCache<number>(1);
		const failedLoad = Promise.reject(new Error("image failed"));

		cache.set("puppet", failedLoad);
		await expect(failedLoad).rejects.toThrow("image failed");
		await Promise.resolve();
		expect(cache.get("puppet")).toBeUndefined();

		cache.set("puppet", Promise.resolve(42));
		expect(await cache.get("puppet")).toBe(42);
	});
});
