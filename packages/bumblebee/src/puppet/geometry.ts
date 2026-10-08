const PUPPET_ART_PLANE_SIZE = 2;
const PUPPET_STICK_BOTTOM_Y = 0;
const PUPPET_STICK_TOP_Y = 3;
const PUPPET_STICK_RADIUS = 0.09;

type PuppetVisibleArtBounds = {
	left: number;
	right: number;
	top: number;
	bottom: number;
	width: number;
	height: number;
};

/** Stick-space anchor shared by live puppets and the offline frame renderer. */
const getPuppetNameplateLocalAnchor = (bounds: Pick<PuppetVisibleArtBounds, "top" | "bottom">) => ({
	x: 0,
	y: bounds.bottom - Math.max(0.001, bounds.top - bounds.bottom) * 0.035
});

const buildCenteredArtPlaneMeasurements = (
	width = PUPPET_ART_PLANE_SIZE,
	height = PUPPET_ART_PLANE_SIZE
) => {
	const top = PUPPET_STICK_TOP_Y + height / 2;
	const bottom = PUPPET_STICK_TOP_Y - height / 2;
	return {
		visibleHeight: top,
		visibleArtBounds: {
			left: -width / 2,
			right: width / 2,
			top,
			bottom,
			width,
			height
		} satisfies PuppetVisibleArtBounds
	};
};

export {
	getPuppetNameplateLocalAnchor,
	type PuppetVisibleArtBounds,
	buildCenteredArtPlaneMeasurements,
	PUPPET_ART_PLANE_SIZE,
	PUPPET_STICK_BOTTOM_Y,
	PUPPET_STICK_RADIUS,
	PUPPET_STICK_TOP_Y
};
