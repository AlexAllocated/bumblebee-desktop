class BoundedPromiseCache<T> {
	private entries = new Map<string, Promise<T>>();

	constructor(
		private readonly maxEntries: number,
		private readonly onEvict?: (entry: T) => void
	) {}

	get(key: string) {
		const entry = this.entries.get(key);
		if (!entry) return undefined;
		this.entries.delete(key);
		this.entries.set(key, entry);
		return entry;
	}

	set(key: string, promise: Promise<T>) {
		this.entries.set(key, promise);
		promise.catch(() => {
			if (this.entries.get(key) === promise) {
				this.entries.delete(key);
			}
		});
		this.trim();
	}

	clear() {
		const entries = Array.from(this.entries.values());
		this.entries.clear();
		if (this.onEvict) {
			for (const promise of entries) {
				promise.then(this.onEvict, () => {});
			}
		}
	}

	private trim() {
		while (this.entries.size > this.maxEntries) {
			const oldest = this.entries.entries().next().value as [string, Promise<T>] | undefined;
			if (!oldest) return;
			const [key, promise] = oldest;
			this.entries.delete(key);
			if (this.onEvict) {
				promise.then(this.onEvict, () => {});
			}
		}
	}
}

export { BoundedPromiseCache };
