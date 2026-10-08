import type { Scene } from "@babylonjs/core/scene";
const sources = new WeakMap<Scene, () => number>();
export const setSceneRandomSeed = (scene: Scene, seed: number) => {
	let state = seed >>> 0;
	sources.set(scene, () => {
		state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
		return state / 4294967296;
	});
};
export const sceneRandom = (scene: Scene) => sources.get(scene)?.() ?? Math.random();
