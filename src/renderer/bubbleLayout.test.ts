import { describe, expect, test } from "bun:test";
import { bubbleCenter, bubbleWidthLimit, wordFragments } from "./bubbleLayout";

describe("speech bubbles in the OBS viewport", () => {
  test("the observed 1280px greeting no longer extends beyond the left edge", () => {
    const viewport = 1280;
    const width = bubbleWidthLimit(viewport);
    // The old 22% anchor with a 50%-width padded bubble produced a negative left edge.
    expect(viewport * 0.22 - (viewport * 0.5 + 46) / 2).toBeLessThan(0);
    const center = bubbleCenter(viewport * 0.15, width, viewport);
    expect(center - width / 2).toBeGreaterThanOrEqual(8);
    expect(center + width / 2).toBeLessThanOrEqual(viewport - 8);
  });

  test("resizing and speakers at either edge retain both viewport gutters", () => {
    for (const viewport of [320, 480, 800, 1280, 1920]) {
      const limit = bubbleWidthLimit(viewport);
      for (const width of [80, limit]) {
        for (const preferred of [
          0,
          viewport * 0.15,
          viewport * 0.7,
          viewport,
        ]) {
          const center = bubbleCenter(preferred, width, viewport);
          expect(center - width / 2).toBeGreaterThanOrEqual(8);
          expect(center + width / 2).toBeLessThanOrEqual(viewport - 8);
        }
      }
    }
  });

  test("Azure punctuation tokens highlight without changing the original greeting", () => {
    const text =
      "Hi, I'm Bumblebee! An interactive companion created by Alex Ford.";
    const tokens = [
      "Hi",
      ",",
      "I'm",
      "Bumblebee",
      "!",
      "An",
      "interactive",
      "companion",
      "created",
      "by",
      "Alex",
      "Ford",
      ".",
    ];
    const result = wordFragments(
      text,
      tokens.map((text) => ({ text })),
    );
    expect(result.map((part) => part.text).join("")).toBe(text);
    expect(result.find((part) => part.word === 5)?.text).toBe("An");
  });

  test("repeated words and missing timing tokens keep all original text intact", () => {
    const text = "Bee, bee! 🐝 Keep  both spaces.";
    const result = wordFragments(
      text,
      ["Bee", "bee", "unknown", "Keep", "both", "spaces"].map((text) => ({
        text,
      })),
    );
    expect(result.map((part) => part.text).join("")).toBe(text);
    expect(
      result.filter((part) => part.word !== null).map((part) => part.word),
    ).toEqual([0, 1, 3, 4, 5]);
  });
});
