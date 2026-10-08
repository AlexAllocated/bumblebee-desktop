/** Retained Honeycomb layout contract. Coordinates and anchors are shared by editor and OBS. */
export const OVERLAY_RECT_ANCHORS = [
  "top-left",
  "top-right",
  "bottom-left",
  "bottom-right",
  "center",
] as const;
export type OverlayRectAnchor = (typeof OVERLAY_RECT_ANCHORS)[number];
export type Position = { horizontalPercent: number; verticalPercent: number };
export interface BubbleSettings {
  chatBubblesEnabled: boolean;
  chatBubbleStyleId: string;
  chatBubbleMaxWidthPercent: number;
  chatBubbleMaxHeightPercent: number;
  chatBubbleTextSizePercentage: number;
}
export interface BumblebeeLayout extends BubbleSettings {
  position: Position;
  anchor: OverlayRectAnchor;
  scalePercentage: number;
  visible: boolean;
}
export interface PuppetLayout extends BubbleSettings {
  enabled: boolean;
  position: { horizontalPercent: number };
  anchor: OverlayRectAnchor;
  scalePercentage: number;
  occlusionPercentage: number;
  showWhenIdle: boolean;
  nameplatesEnabled: boolean;
}

export interface StreamerBubbleLayout {
  enabled: boolean;
  chatBubbleStyleId: string;
  position: Position;
  anchor: OverlayRectAnchor;
  textSizePercentage: number;
  chatBubbleMaxWidthPercent: number;
  chatBubbleMaxHeightPercent: number;
  subjectRadiusPx: number;
  tail: {
    anchorRatioX: number;
    anchorRatioY: number;
    vectorX: number;
    vectorY: number;
  };
}
export interface OverlaySettings {
  bumblebee: BumblebeeLayout;
  puppet: PuppetLayout;
  streamerVoiceBubble: StreamerBubbleLayout;
  editPreview: { puppet: boolean; streamerVoiceBubble: boolean };
}
type DeepPartial<T> = {
  [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K];
};
export type OverlaySettingsPatch = DeepPartial<OverlaySettings>;
export type OverlaySubject = "bumblebee" | "puppet" | "streamerVoiceBubble";
export const subjectLabels: Record<OverlaySubject, string> = {
  bumblebee: "Bumblebee",
  puppet: "Chat Puppets",
  streamerVoiceBubble: "Streamer Voice Bubble",
};
const bubbles: BubbleSettings = {
  chatBubblesEnabled: true,
  chatBubbleStyleId: "classic-comic",
  chatBubbleMaxWidthPercent: 35,
  chatBubbleMaxHeightPercent: 25,
  chatBubbleTextSizePercentage: 0.5,
};
export const defaultOverlaySettings: OverlaySettings = {
  bumblebee: {
    ...bubbles,
    position: { horizontalPercent: 15, verticalPercent: 100 },
    anchor: "bottom-left",
    scalePercentage: 0.2,
    visible: true,
  },
  puppet: {
    ...bubbles,
    enabled: true,
    position: { horizontalPercent: 85 },
    anchor: "center",
    scalePercentage: 0.3,
    occlusionPercentage: 0.3,
    showWhenIdle: false,
    nameplatesEnabled: true,
  },
  streamerVoiceBubble: {
    enabled: false,
    chatBubbleStyleId: "classic-comic",
    position: { horizontalPercent: 25, verticalPercent: 25 },
    anchor: "center",
    textSizePercentage: 0.5,
    chatBubbleMaxWidthPercent: 35,
    chatBubbleMaxHeightPercent: 25,
    subjectRadiusPx: 80,
    tail: { anchorRatioX: 0, anchorRatioY: 1, vectorX: -20, vectorY: 25 },
  },
  editPreview: { puppet: false, streamerVoiceBubble: false },
};
/** Overlay settings are JSON data; this also snapshots reactive Svelte proxies safely. */
export function cloneOverlaySettings(
  settings: OverlaySettings,
): OverlaySettings {
  return JSON.parse(JSON.stringify(settings)) as OverlaySettings;
}
export function applyOverlayPatch(
  settings: OverlaySettings,
  patch: OverlaySettingsPatch,
): OverlaySettings {
  const merge = (base: any, next: any): any =>
    Object.fromEntries(
      Object.keys(base).map((key) => [
        key,
        next?.[key] === undefined
          ? base[key]
          : typeof base[key] === "object" && base[key] !== null
            ? merge(base[key], next[key])
            : next[key],
      ]),
    );
  return cloneOverlaySettings(merge(settings, patch));
}
