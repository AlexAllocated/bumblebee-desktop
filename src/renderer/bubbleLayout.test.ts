import { describe, expect, test } from "bun:test";
import {
  bubbleCenter,
  bubbleWidthLimit,
  bubbleHeightLimit,
  wordFragments,
  wordScrollTop,
} from "./bubbleLayout";

describe("speech bubbles in the OBS viewport", () => {
  test("a long caption stays below the top gutter at small and large source sizes", () => {
    for (const height of [180, 360, 720, 1080]) {
      const bounded = Math.min(4000, bubbleHeightLimit(height));
      expect(height - height * 0.36 - bounded).toBeGreaterThanOrEqual(7.999);
      expect(bounded).toBeLessThan(height);
    }
  });

  test("all 6000 characters remain while timed words follow through a scrollable caption", () => {
    const text = "Bumblebee ".repeat(600);
    expect(text.length).toBe(6000);
    expect(
      wordFragments(
        text,
        Array.from({ length: 600 }, () => ({ text: "Bumblebee" })),
      )
        .map((part) => part.text)
        .join(""),
    ).toBe(text);
    const visibleHeight = bubbleHeightLimit(180) - 6;
    const contentHeight = 14 + 150 * 24 + 14;
    let scroll = 0;
    for (let word = 0; word < 600; word++) {
      const top = 14 + Math.floor(word / 4) * 24;
      scroll = wordScrollTop(scroll, visibleHeight, contentHeight, top, 20);
      expect(top - scroll).toBeGreaterThanOrEqual(8 - 0.001);
      expect(top + 20 - scroll).toBeLessThanOrEqual(visibleHeight - 8 + 0.001);
    }
    expect(scroll).toBeGreaterThan(3000);
  });

  test("resizing and backward seeks keep the active word visible without overscrolling", () => {
    const resized = wordScrollTop(800, 100, 2400, 1040, 22);
    expect(1040 + 22 - resized).toBeLessThanOrEqual(92);
    const rewind = wordScrollTop(resized, 100, 2400, 14, 22);
    expect(14 - rewind).toBeGreaterThanOrEqual(8);
    expect(wordScrollTop(0, 300, 80, 14, 22)).toBe(0);
    // A single wrapped timing token taller than the viewport keeps its beginning visible.
    expect(wordScrollTop(0, 100, 2400, 1200, 150)).toBe(1192);
  });
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
