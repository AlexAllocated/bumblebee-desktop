import "@babylonjs/loaders/glTF/2.0";
import { AnimationGroup } from "@babylonjs/core/Animations/animationGroup";
import { BoundingInfo } from "@babylonjs/core/Culling/boundingInfo";
import { ImportMeshAsync } from "@babylonjs/core/Loading/sceneLoader";
import { Matrix, Quaternion, Vector3 } from "@babylonjs/core/Maths/math.vector";
import { Scene } from "@babylonjs/core/scene";
import { TransformNode } from "@babylonjs/core/Meshes/transformNode";
import z from "zod";
import { loadBumblebeeModelSource } from "./modelAsset";

const animationNameMap = {
	"A backflip": "backflip",
	"A error": "error",
	"A FlyingActive-loop": "activeFlying",
	"A FlyingIdle-loop": "idleFlying",
	"A IdleStanding-loop": "standing",
	"A StandingTalking-loop": "standingTalking",
	"A Walking-loop": "walking",
	"FA mad": "mad",
	"FA mischief": "mischief",
	"FA roundEyes": "roundEyes",
	"FA sad": "sad",
	"FA smileEyes": "smileEyes",
	"FA smileEyesU": "smileEyesU",
	"FA talking-loop": "talking",
	"FA winkLeft": "winkLeft",
	"FA winkRight": "winkRight"
};

const OptionsSchema = z.object({
	scene: z.instanceof(Scene),
	modelUrl: z.string().optional()
});

type Options = z.input<typeof OptionsSchema>;

const additiveAnimationNames = new Set([
	"backflip",
	"mad",
	"mischief",
	"roundEyes",
	"sad",
	"smileEyes",
	"smileEyesU",
	"standingTalking",
	"talking",
	"walking",
	"winkLeft",
	"winkRight"
]);

const facialAnimationNames = new Set([
	"mad",
	"mischief",
	"roundEyes",
	"sad",
	"smileEyes",
	"smileEyesU",
	"talking",
	"winkLeft",
	"winkRight"
]);

const facialAnimationTargetNames = new Set(["mouthDriver", "eyesDriver", "colorDriver"]);

const isFacialAnimationTarget = (target: { name: string; getClassName?: () => string }) =>
	facialAnimationTargetNames.has(target.name) ||
	target.name.startsWith("tenna") ||
	target.getClassName?.() === "MorphTarget";

const pruneBumblebeeFacialAnimationTargets = (animationGroup: AnimationGroup) => {
	if (!facialAnimationNames.has(animationGroup.name)) return;
	for (const targetedAnimation of [...animationGroup.targetedAnimations]) {
		if (isFacialAnimationTarget(targetedAnimation.target)) {
			continue;
		}
		animationGroup.removeTargetedAnimation(targetedAnimation.animation);
	}
};

const createAdditiveAnimationGroup = (animationGroup: AnimationGroup) => {
	const additiveGroup = animationGroup.clone(animationGroup.name, undefined, true, true);
	AnimationGroup.MakeAnimationAdditive(additiveGroup, {
		referenceFrame: additiveGroup.from ?? 0
	});
	return additiveGroup;
};

const frontFootNodeNames = ["legFL", "legFR"];

const loadModel = async (options: Options) => {
	const { scene, modelUrl } = OptionsSchema.parse(options);
	const camera = scene.activeCamera!;

	const resolvedModelUrl = modelUrl ?? "/models/bumblebee.cb67e11b.glb";
	const modelSource = await loadBumblebeeModelSource(resolvedModelUrl);
	const { meshes, animationGroups: animationGroups } = await ImportMeshAsync(
		modelSource,
		scene!,
		typeof modelSource === "string"
			? undefined
			: { name: resolvedModelUrl, pluginExtension: ".glb" }
	);
	const frontFootNodes = frontFootNodeNames
		.map((name) => scene.getTransformNodeByName(name))
		.filter((node): node is TransformNode => Boolean(node));

	const mesh = meshes[0];
	mesh.scaling.setAll(1);

	// Compute aggregate bounding info from all child meshes.
	const { min, max } = mesh.getHierarchyBoundingVectors();
	const localSize = max.subtract(min);
	const center = Vector3.Center(min, max);
	const bottomCenter = new Vector3((min.x + max.x) / 2, min.y, (min.z + max.z) / 2);
	mesh.setPivotMatrix(Matrix.Translation(-center.x, -center.y, -center.z));
	const node = new TransformNode("pivot", scene);
	node.metadata = {
		localDims: {
			w: localSize.x,
			h: localSize.y
		}
	};
	node.position = bottomCenter;
	mesh.rotationQuaternion = Quaternion.Identity();
	const boundingInfo = new BoundingInfo(min, max);
	mesh.setBoundingInfo(boundingInfo);
	mesh.setParent(node);
	const viewToCamera = Matrix.LookAtLH(Vector3.Zero(), camera.position, camera.upVector);
	const worldFromViewCamera = viewToCamera.invert();
	const cameraQuaternion = Quaternion.FromRotationMatrix(worldFromViewCamera);
	node.rotationQuaternion = cameraQuaternion;

	// Rename and organize built-in animations. Animations that layer over the
	// current pose are cloned and converted up front because Babylon mutates
	// animation keys when making a group additive.
	const playableAnimationGroups = animationGroups.map((animationGroup) => {
		const animationName = animationGroup.name as keyof typeof animationNameMap;
		if (animationNameMap[animationName]) {
			animationGroup.name = animationNameMap[animationName];
		}
		animationGroup.stop();
		pruneBumblebeeFacialAnimationTargets(animationGroup);

		if (!additiveAnimationNames.has(animationGroup.name)) return animationGroup;

		const additiveGroup = createAdditiveAnimationGroup(animationGroup);
		animationGroup.dispose();
		additiveGroup.stop();
		return additiveGroup;
	});

	return { node, animationGroups: playableAnimationGroups, frontFootNodes };
};

export { loadModel, pruneBumblebeeFacialAnimationTargets };
