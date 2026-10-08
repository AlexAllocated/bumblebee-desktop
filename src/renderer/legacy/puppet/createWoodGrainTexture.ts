import { Color3 } from "@babylonjs/core/Maths/math.color";
import { DynamicTexture } from "@babylonjs/core/Materials/Textures/dynamicTexture";
import { Texture } from "@babylonjs/core/Materials/Textures/texture";
import type { Scene } from "@babylonjs/core/scene";
import { sceneRandom } from "../utils/sceneRandom";

/**
 * Creates a simple wood‑grain DynamicTexture:
 *   - base fill
 *   - random semi‑transparent streaks for grain
 */
const createWoodGrainTexture = (
	name: string,
	width: number,
	height: number,
	baseColor: string,
	scene: Scene
): DynamicTexture => {
	const dynamicTexture = new DynamicTexture(name, { width, height }, scene, false);
	dynamicTexture.wrapU = Texture.CLAMP_ADDRESSMODE;
	dynamicTexture.wrapV = Texture.CLAMP_ADDRESSMODE;
	const context2D = dynamicTexture.getContext();

	// base color
	context2D.fillStyle = baseColor;
	context2D.fillRect(0, 0, width, height);

	// grain streaks
	const random = () => sceneRandom(scene);
	for (let i = 0; i < 100; i++) {
		const x = random() * width;
		const y = random() * height;
		const len = height * (0.4 + random() * 0.2);
		const thickness = 30 + random() * 2;
		const dark = Color3.FromHexString(baseColor).scale(0.8);
		context2D.strokeStyle = `rgba(${dark.r * 255},${dark.g * 255},${dark.b * 255},${0.05 + random() * 0.25})`;
		context2D.lineWidth = thickness;
		context2D.beginPath();
		context2D.moveTo(x, y);
		context2D.lineTo(x + (random() * 8 - 4), y + len);
		context2D.stroke();
	}

	dynamicTexture.update();
	return dynamicTexture;
};

export { createWoodGrainTexture };
