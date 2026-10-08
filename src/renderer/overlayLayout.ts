import {
  cloneOverlaySettings,
  type OverlaySettings,
  OverlaySubject,
  OverlaySettingsPatch,
  OverlayRectAnchor,
} from "../lib/overlay";
import { fittedActorSize } from "./legacyLayout";
import {
  overlayRectFromAnchorPosition,
  overlayRectPositionFromRect,
  overlayRectAnchorFromRect,
  type OverlayRect,
  type OverlayRectViewport,
} from "./overlayRectAnchor";
import {
  addPoints,
  normalizeVector,
  pointDistance,
  scalePoint,
  subtractPoints,
  subjectCircleForPose,
} from "@hivetech/speech-bubbles";

export { subjectCircleForPose };

export type Viewport = OverlayRectViewport;
export type Dimensions = {
  bee: { w: number; h: number };
  puppetAspect: number;
};
export const defaultDimensions: Dimensions = {
  bee: { w: 1, h: 1 },
  puppetAspect: 0.72,
};
export const clamp = (n: number, min: number, max: number) =>
  min <= max ? Math.max(min, Math.min(max, n)) : (min + max) / 2;
export function rect(
  left: number,
  top: number,
  width: number,
  height: number,
): OverlayRect {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    centerX: left + width / 2,
    centerY: top + height / 2,
  };
}
export function subjectEnabled(
  settings: OverlaySettings,
  subject: OverlaySubject,
) {
  return subject === "bumblebee"
    ? settings.bumblebee.visible
    : settings[subject].enabled;
}
/** Exact retained fit/anchor and bottom-occlusion formulas from Honeycomb's overlay editor. */
export function subjectRect(
  settings: OverlaySettings,
  subject: OverlaySubject,
  viewport: Viewport,
  dimensions = defaultDimensions,
): OverlayRect {
  if (subject === "bumblebee") {
    const s = settings.bumblebee;
    const fitted = fittedActorSize(viewport, dimensions.bee, s.scalePercentage);
    return overlayRectFromAnchorPosition(
      s.position,
      s.anchor,
      { width: (fitted.width * 4) / 3, height: (fitted.height * 4) / 3 },
      viewport,
    );
  }
  if (subject === "streamerVoiceBubble") {
    const s = settings.streamerVoiceBubble;
    return overlayRectFromAnchorPosition(
      s.position,
      s.anchor,
      {
        width: (viewport.width * s.chatBubbleMaxWidthPercent) / 100,
        height: (viewport.height * s.chatBubbleMaxHeightPercent) / 100,
      },
      viewport,
    );
  }
  const s = settings[subject];
  const height = viewport.height * s.scalePercentage;
  const width = height * dimensions.puppetAspect;
  const anchorX = (viewport.width * s.position.horizontalPercent) / 100;
  const left =
    anchorX -
    (s.anchor.includes("right")
      ? width
      : s.anchor === "center"
        ? width / 2
        : 0);
  return rect(
    left,
    viewport.height * (1 + (s.occlusionPercentage - 1) * s.scalePercentage),
    width,
    height,
  );
}
export function bottomAnchor(
  r: OverlayRect,
  viewport: Viewport,
): OverlayRectAnchor {
  const percent = (r.centerX / viewport.width) * 100;
  return percent >= 25 && percent <= 75
    ? "center"
    : percent < 50
      ? "bottom-left"
      : "bottom-right";
}
export function placementPatch(
  settings: OverlaySettings,
  subject: OverlaySubject,
  wanted: OverlayRect,
  viewport: Viewport,
): OverlaySettingsPatch {
  const left = clamp(wanted.left, 0, viewport.width - wanted.width);
  if (subject === "puppet") {
    const s = settings[subject];
    const scale = s.scalePercentage;
    const occlusion = clamp(
      (wanted.centerY / viewport.height - 1) / scale + 0.5,
      0.1,
      0.5,
    );
    const r = rect(
      left,
      viewport.height * (1 + (occlusion - 1) * scale),
      wanted.width,
      wanted.height,
    );
    const anchor = bottomAnchor(r, viewport);
    return {
      [subject]: {
        position: {
          horizontalPercent: overlayRectPositionFromRect(r, anchor, viewport)
            .horizontalPercent,
        },
        anchor,
        occlusionPercentage: occlusion,
      },
    };
  }
  let top = clamp(wanted.top, 0, viewport.height - wanted.height);
  // Bumblebee lands when the bottom of his authored bounds enters the bottom magnet zone.
  if (subject === "bumblebee" && top + wanted.height >= viewport.height * 0.97)
    top = viewport.height - wanted.height;
  const r = rect(left, top, wanted.width, wanted.height);
  const anchor = overlayRectAnchorFromRect(r, viewport);
  return {
    [subject]: {
      position: overlayRectPositionFromRect(r, anchor, viewport),
      anchor,
    },
  };
}
export type ResizeDirection = "nw" | "ne" | "sw" | "se";
/** Retained opposite-edge resize ratio; actor aspect stays fixed. */
export function resizePatch(
  settings: OverlaySettings,
  subject: OverlaySubject,
  start: OverlayRect,
  direction: ResizeDirection,
  point: { x: number; y: number },
  viewport: Viewport,
  dimensions = defaultDimensions,
): OverlaySettingsPatch {
  const width = direction.includes("e")
    ? point.x - start.left
    : start.right - point.x;
  const height = direction.includes("s")
    ? point.y - start.top
    : start.bottom - point.y;
  if (subject === "streamerVoiceBubble") {
    const w = clamp((width / viewport.width) * 100, 15, 70);
    const h = clamp((height / viewport.height) * 100, 10, 70);
    const size = {
      width: (viewport.width * w) / 100,
      height: (viewport.height * h) / 100,
    };
    const next = rect(
      direction.includes("w") ? start.right - size.width : start.left,
      direction.includes("n") ? start.bottom - size.height : start.top,
      size.width,
      size.height,
    );
    return {
      streamerVoiceBubble: {
        ...placementPatch(settings, subject, next, viewport)
          .streamerVoiceBubble,
        chatBubbleMaxWidthPercent: w,
        chatBubbleMaxHeightPercent: h,
      },
    };
  }
  const ratio = Math.max(
    width / Math.max(start.width, 1),
    height / Math.max(start.height, 1),
  );
  const scale = clamp(settings[subject].scalePercentage * ratio, 0.05, 1);
  const nextSettings = cloneOverlaySettings(settings);
  nextSettings[subject].scalePercentage = scale;
  const size = subjectRect(nextSettings, subject, viewport, dimensions);
  const next = rect(
    direction.includes("w") ? start.right - size.width : start.left,
    direction.includes("n") ? start.bottom - size.height : start.top,
    size.width,
    size.height,
  );
  return {
    [subject]: {
      ...placementPatch(nextSettings, subject, next, viewport)[subject],
      scalePercentage: scale,
    },
  };
}
export function streamerPose(settings: OverlaySettings, viewport: Viewport) {
  const body = subjectRect(settings, "streamerVoiceBubble", viewport);
  const s = settings.streamerVoiceBubble;
  return {
    anchor: {
      x: body.left + body.width * s.tail.anchorRatioX,
      y: body.top + body.height * s.tail.anchorRatioY,
    },
    tailVector: { x: s.tail.vectorX, y: s.tail.vectorY },
    subjectRadius: s.subjectRadiusPx,
  };
}
export function tailPatch(
  settings: OverlaySettings,
  point: { x: number; y: number },
  viewport: Viewport,
): OverlaySettingsPatch {
  const body = subjectRect(settings, "streamerVoiceBubble", viewport);
  const anchorRatioX = clamp((point.x - body.left) / body.width, 0, 1);
  const anchorRatioY = clamp((point.y - body.top) / body.height, 0, 1);
  return {
    streamerVoiceBubble: {
      tail: {
        anchorRatioX,
        anchorRatioY,
        vectorX: clamp(
          point.x - body.left - body.width * anchorRatioX,
          -1000,
          1000,
        ),
        vectorY: clamp(
          point.y - body.top - body.height * anchorRatioY,
          -1000,
          1000,
        ),
      },
    },
  };
}

/** Retained subject-circle editor math from speech-bubbles/placeholder.ts. */
export function streamerSubjectPatch(
  settings: OverlaySettings,
  wanted: { x: number; y: number },
  requestedRadius: number,
  viewport: Viewport,
): OverlaySettingsPatch {
  const body = subjectRect(settings, "streamerVoiceBubble", viewport);
  const radius = clamp(requestedRadius, 8, 600);
  const nearestEdge = (point: { x: number; y: number }) => {
    const x = clamp(point.x, body.left, body.right),
      y = clamp(point.y, body.top, body.bottom);
    if (
      point.x < body.left ||
      point.x > body.right ||
      point.y < body.top ||
      point.y > body.bottom
    ) {
      const edge = { x, y };
      return {
        point: edge,
        direction: normalizeVector(subtractPoints(point, edge)),
        distance: pointDistance(point, edge),
      };
    }
    return [
      {
        point: { x: body.left, y: point.y },
        direction: { x: -1, y: 0 },
        distance: point.x - body.left,
      },
      {
        point: { x: body.right, y: point.y },
        direction: { x: 1, y: 0 },
        distance: body.right - point.x,
      },
      {
        point: { x: point.x, y: body.top },
        direction: { x: 0, y: -1 },
        distance: point.y - body.top,
      },
      {
        point: { x: point.x, y: body.bottom },
        direction: { x: 0, y: 1 },
        distance: body.bottom - point.y,
      },
    ].reduce((best, candidate) =>
      candidate.distance < best.distance ? candidate : best,
    );
  };
  const edge = nearestEdge(wanted);
  const center =
    edge.distance < radius + 16
      ? addPoints(edge.point, scalePoint(edge.direction, radius + 16))
      : wanted;
  const { point: anchor, direction } = nearestEdge(center);
  const tip = addPoints(center, scalePoint(direction, -radius));
  return {
    streamerVoiceBubble: {
      subjectRadiusPx: radius,
      tail: {
        anchorRatioX: (anchor.x - body.left) / body.width,
        anchorRatioY: (anchor.y - body.top) / body.height,
        vectorX: tip.x - anchor.x,
        vectorY: tip.y - anchor.y,
      },
    },
  };
}
