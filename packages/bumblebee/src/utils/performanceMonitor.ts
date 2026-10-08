interface PerformanceStats {
	fps: number;
	memoryUsed: number;
	totalJSHeapSize: number;
	usedJSHeapSize: number;
	renderTime: number;
	deltaTime: number;
}

type PerformanceWithMemory = Performance & {
	memory?: {
		usedJSHeapSize: number;
		totalJSHeapSize: number;
	};
};

class PerformanceMonitor {
	private frameCount = 0;
	private lastFPSUpdate = 0;
	private currentFPS = 0;
	private frameStartTime = 0;
	private renderTimes: number[] = [];
	private maxRenderSamples = 60;
	private onStatsCallback?: (stats: PerformanceStats) => void;
	private updateInterval = 1000; // Update every second

	start(onStats?: (stats: PerformanceStats) => void): void {
		this.onStatsCallback = onStats;
		this.lastFPSUpdate = performance.now();
		this.frameStartTime = performance.now();
	}

	startFrame(): void {
		this.frameStartTime = performance.now();
	}

	endFrame(): void {
		const now = performance.now();
		const renderTime = now - this.frameStartTime;

		// Track render times
		this.renderTimes.push(renderTime);
		if (this.renderTimes.length > this.maxRenderSamples) {
			this.renderTimes.shift();
		}

		this.frameCount++;

		// Update FPS every second
		if (now - this.lastFPSUpdate >= this.updateInterval) {
			this.currentFPS = Math.round((this.frameCount * 1000) / (now - this.lastFPSUpdate));
			this.frameCount = 0;
			this.lastFPSUpdate = now;

			// Report stats if callback provided
			if (this.onStatsCallback) {
				this.onStatsCallback(this.getStats());
			}
		}
	}

	getStats(): PerformanceStats {
		const memory = (performance as PerformanceWithMemory).memory;
		const avgRenderTime =
			this.renderTimes.length > 0
				? this.renderTimes.reduce((a, b) => a + b, 0) / this.renderTimes.length
				: 0;

		return {
			fps: this.currentFPS,
			memoryUsed: memory ? memory.usedJSHeapSize : 0,
			totalJSHeapSize: memory ? memory.totalJSHeapSize : 0,
			usedJSHeapSize: memory ? memory.usedJSHeapSize : 0,
			renderTime: avgRenderTime,
			deltaTime: this.renderTimes.length > 0 ? this.renderTimes[this.renderTimes.length - 1] : 0
		};
	}

	getFPS(): number {
		return this.currentFPS;
	}

	stop(): void {
		this.onStatsCallback = undefined;
	}
}

// Global performance monitor instance
const performanceMonitor = new PerformanceMonitor();

// Export functions for use in render manager
export function startPerformanceMonitoring(onStats?: (stats: PerformanceStats) => void): void {
	performanceMonitor.start(onStats);
}

export function startFrame(): void {
	performanceMonitor.startFrame();
}

export function endFrame(): void {
	performanceMonitor.endFrame();
}

export function getPerformanceStats(): PerformanceStats {
	return performanceMonitor.getStats();
}

export function stopPerformanceMonitoring(): void {
	performanceMonitor.stop();
}

export type { PerformanceStats };
export { PerformanceMonitor };
