import { describe, expect, test } from "bun:test";
import {
  defaultOverlaySettings,
  applyOverlayPatch,
  cloneOverlaySettings,
} from "../lib/overlay";
import {
  subjectRect,
  placementPatch,
  resizePatch,
  rect,
  streamerPose,
  tailPatch,
  streamerSubjectPatch,
  subjectCircleForPose,
} from "./overlayLayout";

describe("retained overlay editor geometry", () => {
  test("reactive settings can enter editor and renderer without structured-clone errors", () => {
    const source = structuredClone(defaultOverlaySettings);
    const proxied = new Proxy(source, {});
    expect(() => structuredClone(proxied)).toThrow();
    const copy = cloneOverlaySettings(proxied);
    const changed = applyOverlayPatch(proxied, {
      bumblebee: { visible: false },
    });
    copy.bumblebee.position.horizontalPercent = 90;
    expect(source.bumblebee.position.horizontalPercent).toBe(15);
    expect(changed.bumblebee.visible).toBe(false);
    expect(() => structuredClone(changed)).not.toThrow();
  });
  test("all three subjects retain edge anchors between preview and OBS viewport sizes", () => {
    const s = structuredClone(defaultOverlaySettings);
    s.bumblebee.anchor = "bottom-right";
    s.bumblebee.position = { horizontalPercent: 100, verticalPercent: 100 };
    s.puppet.anchor = "bottom-left";
    s.puppet.position.horizontalPercent = 0;
    s.streamerVoiceBubble.anchor = "top-right";
    s.streamerVoiceBubble.position = {
      horizontalPercent: 100,
      verticalPercent: 0,
    };
    for (const v of [
      { width: 1920, height: 1080 },
      { width: 640, height: 360 },
      { width: 800, height: 600 },
    ]) {
      expect(subjectRect(s, "bumblebee", v).right).toBeCloseTo(v.width);
      expect(subjectRect(s, "bumblebee", v).bottom).toBeCloseTo(v.height);
      expect(subjectRect(s, "puppet", v).left).toBeCloseTo(0);
      expect(subjectRect(s, "streamerVoiceBubble", v).right).toBeCloseTo(
        v.width,
      );
      expect(subjectRect(s, "streamerVoiceBubble", v).top).toBeCloseTo(0);
    }
  });
  test("moving a puppet changes bottom occlusion without letting its slot leave the stage", () => {
    const s = structuredClone(defaultOverlaySettings);
    const v = { width: 1920, height: 1080 };
    const old = subjectRect(s, "puppet", v);
    const next = applyOverlayPatch(
      s,
      placementPatch(
        s,
        "puppet",
        rect(-200, old.top - 200, old.width, old.height),
        v,
      ),
    );
    expect(next.puppet.occlusionPercentage).toBe(0.1);
    expect(subjectRect(next, "puppet", v).left).toBeCloseTo(0);
  });
  test("corner resize preserves the opposite corner and the actor aspect", () => {
    const s = structuredClone(defaultOverlaySettings);
    const v = { width: 1920, height: 1080 };
    s.bumblebee.position = { horizontalPercent: 50, verticalPercent: 50 };
    s.bumblebee.anchor = "center";
    const old = subjectRect(s, "bumblebee", v);
    const p = resizePatch(
      s,
      "bumblebee",
      old,
      "nw",
      { x: old.left - 40, y: old.top - 40 },
      v,
    );
    const next = subjectRect(applyOverlayPatch(s, p), "bumblebee", v);
    expect(next.right).toBeCloseTo(old.right);
    expect(next.bottom).toBeCloseTo(old.bottom);
    expect(next.width / next.height).toBeCloseTo(old.width / old.height);
  });
  test("detached bubble has independent width/height and a saved tail tip", () => {
    const s = structuredClone(defaultOverlaySettings);
    const v = { width: 1920, height: 1080 };
    const old = subjectRect(s, "streamerVoiceBubble", v);
    const resized = applyOverlayPatch(
      s,
      resizePatch(
        s,
        "streamerVoiceBubble",
        old,
        "se",
        { x: old.right + 100, y: old.bottom + 20 },
        v,
      ),
    );
    const box = subjectRect(resized, "streamerVoiceBubble", v);
    expect(box.left).toBeCloseTo(old.left);
    expect(box.top).toBeCloseTo(old.top);
    const tip = { x: box.left - 50, y: box.bottom + 50 };
    const updated = applyOverlayPatch(resized, tailPatch(resized, tip, v));
    const pose = streamerPose(updated, v);
    expect(pose.anchor.x + pose.tailVector.x).toBeCloseTo(tip.x);
    expect(pose.anchor.y + pose.tailVector.y).toBeCloseTo(tip.y);
  });
  test("sparse tail edits preserve all other fields and do not resurrect excluded QR settings", () => {
    const s = structuredClone(defaultOverlaySettings);
    s.puppet.enabled = false;
    const next = applyOverlayPatch(s, {
      streamerVoiceBubble: { tail: { vectorX: 24 } },
    });
    expect(next.streamerVoiceBubble.tail.vectorX).toBe(24);
    expect(next.streamerVoiceBubble.tail.vectorY).toBe(
      s.streamerVoiceBubble.tail.vectorY,
    );
    expect(next.puppet.enabled).toBe(false);
    expect("promotionQrCode" in next).toBe(false);
  });
  test("moving and resizing the camera target preserves the retained body gap and saved circle", () => {
    const s = structuredClone(defaultOverlaySettings),
      v = { width: 1280, height: 720 };
    const center = { x: 700, y: 500 };
    const next = applyOverlayPatch(s, streamerSubjectPatch(s, center, 110, v));
    const circle = subjectCircleForPose(streamerPose(next, v));
    expect(circle.center.x).toBeCloseTo(center.x);
    expect(circle.center.y).toBeCloseTo(center.y);
    expect(circle.radius).toBe(110);
    const body = subjectRect(s, "streamerVoiceBubble", v);
    const inside = applyOverlayPatch(
      s,
      streamerSubjectPatch(s, { x: body.centerX, y: body.centerY }, 80, v),
    );
    const adjusted = subjectCircleForPose(streamerPose(inside, v));
    expect(adjusted.center.y + adjusted.radius).toBeLessThanOrEqual(
      body.top - 16,
    );
  });
});
