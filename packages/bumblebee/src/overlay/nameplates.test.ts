import { describe, expect, test } from "bun:test";
class FakeClassList {
  readonly names = new Set<string>();

  add(...names: string[]) {
    for (const name of names) this.names.add(name);
  }

  contains(name: string) {
    return this.names.has(name);
  }

  toggle(name: string, force?: boolean) {
    const enabled = force ?? !this.names.has(name);
    if (enabled) this.names.add(name);
    else this.names.delete(name);
    return enabled;
  }
}

class FakeStyle {
  readonly values = new Map<string, string>();
  color = "";
  zIndex = "";
  transform = "";
  position = "";

  setProperty(name: string, value: string) {
    this.values.set(name, value);
  }

  removeProperty(name: string) {
    this.values.delete(name);
  }
}

class FakeElement {
  readonly classList = new FakeClassList();
  readonly style = new FakeStyle();
  readonly dataset: Record<string, string> = {};
  readonly attributes = new Map<string, string>();
  readonly children: FakeElement[] = [];
  offsetWidth = 0;
  offsetHeight = 0;
  rectLeft = 0;
  rectTop = 0;
  parent: FakeElement | null = null;
  className = "";
  textContent: string | null = null;

  constructor(readonly tagName: string) {}

  set innerHTML(_value: string) {
    throw new Error("innerHTML should not be used by nameplate rendering.");
  }

  append(...children: FakeElement[]) {
    for (const child of children) {
      child.remove();
      child.parent = this;
      this.children.push(child);
    }
  }

  appendChild(child: FakeElement) {
    this.append(child);
    return child;
  }

  replaceChildren(...children: FakeElement[]) {
    for (const child of this.children) child.parent = null;
    this.children.splice(0, this.children.length);
    this.append(...children);
  }

  remove() {
    if (!this.parent) return;
    const index = this.parent.children.indexOf(this);
    if (index >= 0) this.parent.children.splice(index, 1);
    this.parent = null;
  }

  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }

  getBoundingClientRect() {
    return {
      left: this.rectLeft,
      right: this.rectLeft + this.offsetWidth,
      top: this.rectTop,
      bottom: this.rectTop + this.offsetHeight,
      width: this.offsetWidth,
      height: this.offsetHeight,
    };
  }
}

const hasClass = (element: FakeElement, className: string) =>
  element.classList.contains(className) ||
  element.className.split(/\s+/u).includes(className);

const findByClass = (
  root: FakeElement,
  className: string,
): FakeElement | null => {
  if (hasClass(root, className)) return root;
  for (const child of root.children) {
    const match = findByClass(child, className);
    if (match) return match;
  }
  return null;
};

const findByTag = (root: FakeElement, tagName: string): FakeElement | null => {
  if (root.tagName.toLowerCase() === tagName.toLowerCase()) return root;
  for (const child of root.children) {
    const match = findByTag(child, tagName);
    if (match) return match;
  }
  return null;
};

const installDocumentHarness = () => {
  const previousDocument = Object.getOwnPropertyDescriptor(
    globalThis,
    "document",
  );
  const body = new FakeElement("body");
  const document = {
    body,
    createElement: (tagName: string) => new FakeElement(tagName),
    createElementNS: (_namespace: string, tagName: string) =>
      new FakeElement(tagName),
  };

  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: document,
  });

  return {
    body,
    restore() {
      if (previousDocument)
        Object.defineProperty(globalThis, "document", previousDocument);
      else delete (globalThis as Record<string, unknown>).document;
    },
  };
};

const installViewportHarness = (width: number, height: number) => {
  const previousWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { innerWidth: width, innerHeight: height },
  });

  return {
    restore() {
      if (previousWindow)
        Object.defineProperty(globalThis, "window", previousWindow);
      else delete (globalThis as Record<string, unknown>).window;
    },
  };
};

describe("nameplate rendering", () => {
  test("marks existing and future animated themes as playback paused", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();
      nameplates.show("first", { text: "Alice" }, { x: 120, y: 90 });
      nameplates.setPaused(true);
      expect(hasClass(dom.body.children[0]!, "is-playback-paused")).toBe(true);

      nameplates.show("second", { text: "Bob" }, { x: 180, y: 90 });
      expect(hasClass(dom.body.children[1]!, "is-playback-paused")).toBe(true);
      nameplates.setPaused(false);
      expect(
        dom.body.children.every(
          (element) => !element.classList.contains("is-playback-paused"),
        ),
      ).toBe(true);
      nameplates.show("third", { text: "Carol" }, { x: 240, y: 90 });
      expect(hasClass(dom.body.children[2]!, "is-playback-paused")).toBe(false);
    } finally {
      dom.restore();
    }
  });

  test("scales desktop-authored nameplates into an embedded video viewport", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const videoStage = new FakeElement("div");
      videoStage.offsetWidth = 390;
      videoStage.offsetHeight = 219;
      const nameplates = new Nameplates({
        container: videoStage as unknown as HTMLElement,
        designViewport: { width: 1920, height: 1080 },
        scale: 1.2,
      });

      nameplates.show(
        "viewer",
        { text: "Alice" },
        { x: 195, y: 180, scale: 1 },
      );
      const element = videoStage.children[0]!;

      expect(element.style.transform).toContain("scale(0.243)");
      expect(element.style.values.get("--bb-nameplate-font-size")).toBe("18px");

      nameplates.updateOptions({ designViewport: null, scale: 1 });
      expect(element.style.transform).toContain("scale(1.000)");
      expect(element.style.values.has("--bb-nameplate-font-size")).toBe(false);
    } finally {
      dom.restore();
    }
  });

  test("retargets existing and future nameplates into a clipped presentation container", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();
      nameplates.show(
        "viewer",
        { text: "Alice" },
        { x: 120, y: 90, visible: true },
      );
      const element = dom.body.children[0]!;
      const crtClip = new FakeElement("div");

      nameplates.setContainer(crtClip as unknown as HTMLElement);
      expect(dom.body.children).toHaveLength(0);
      expect(crtClip.children).toEqual([element]);

      nameplates.setContainer(null);
      expect(crtClip.children).toHaveLength(0);
      expect(dom.body.children).toEqual([element]);
    } finally {
      dom.restore();
    }
  });

  test("converts viewport anchors into clipped-container coordinates", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const videoStage = new FakeElement("div");
      videoStage.rectLeft = 75;
      videoStage.rectTop = 100;
      videoStage.offsetWidth = 900;
      videoStage.offsetHeight = 510;
      const nameplates = new Nameplates({
        container: videoStage as unknown as HTMLElement,
      });

      nameplates.show(
        "viewer",
        { text: "Alice" },
        { x: 800, y: 560, visible: true },
      );
      const element = videoStage.children[0]!;
      element.offsetWidth = 120;
      element.offsetHeight = 40;
      nameplates.updateGeometry("viewer", { x: 800, y: 560, visible: true });

      expect(element.style.position).toBe("absolute");
      expect(element.style.transform).toContain("translate3d(725px, 464px, 0)");
    } finally {
      dom.restore();
    }
  });

  test("renders display names as text nodes instead of parsed HTML", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();
      const displayName = `<img src=x onerror=alert(1)> Alice & Bob`;

      nameplates.show(
        "viewer",
        { text: displayName, platform: "youtube" },
        { x: 120, y: 90, visible: true },
      );

      const element = dom.body.children[0];
      expect(element?.dataset.visible).toBe("true");
      expect(
        findByClass(dom.body, "bumblebee-nameplate__text")?.textContent,
      ).toBe(displayName);
      expect(findByTag(dom.body, "img")).toBeNull();
      expect(
        findByClass(dom.body, "bumblebee-nameplate__icon")?.attributes.get(
          "aria-label",
        ),
      ).toBe("YouTube");

      nameplates.dispose();
      expect(dom.body.children).toHaveLength(0);
    } finally {
      dom.restore();
    }
  });

  test("ignores unknown platform values from untyped callers", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();

      nameplates.show(
        "viewer",
        { text: "Alice", platform: "unknown" as never },
        { x: 120, y: 90, visible: true },
      );

      expect(
        findByClass(dom.body, "bumblebee-nameplate__text")?.textContent,
      ).toBe("Alice");
      expect(findByClass(dom.body, "bumblebee-nameplate__icon")).toBeNull();
    } finally {
      dom.restore();
    }
  });

  test("slides below-viewport nameplates upward into view", async () => {
    const dom = installDocumentHarness();
    const viewport = installViewportHarness(320, 240);
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();

      nameplates.show(
        "viewer",
        { text: "Alice" },
        { x: 160, y: 290, visible: true },
      );
      const element = dom.body.children[0]!;
      element.offsetWidth = 120;
      element.offsetHeight = 40;

      nameplates.updateGeometry("viewer", { x: 160, y: 290, visible: true });

      expect(element.style.transform).toContain("translate3d(160px, 194px, 0)");
    } finally {
      viewport.restore();
      dom.restore();
    }
  });

  test("allows a hiding puppet's nameplate to follow it beyond the viewport", async () => {
    const dom = installDocumentHarness();
    const viewport = installViewportHarness(320, 240);
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();

      nameplates.show(
        "viewer",
        { text: "Alice" },
        { x: 160, y: 200, visible: true },
      );
      const element = dom.body.children[0]!;
      element.offsetWidth = 120;
      element.offsetHeight = 40;

      nameplates.updateGeometry("viewer", {
        x: 160,
        y: 290,
        visible: true,
        constrainToViewport: false,
      });

      expect(element.style.transform).toContain("translate3d(160px, 296px, 0)");
    } finally {
      viewport.restore();
      dom.restore();
    }
  });

  test("accepts the full shared nameplate decoration and motion contract", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();

      nameplates.show(
        "viewer",
        {
          text: "Alice",
          style: {
            background: "#ffffff",
            border: "#111111",
            text: "#222222",
            accent: "#ffcc00",
            shadow: "rgba(0, 0, 0, 0.25)",
            fontFamily: "rounded",
            shape: "capsule",
            decoration: "waveform",
            motion: "scan",
          },
        },
        { x: 120, y: 90, visible: true },
      );

      const element = dom.body.children[0];
      expect(
        hasClass(element!, "bumblebee-nameplate--decoration-waveform"),
      ).toBe(true);
      expect(hasClass(element!, "bumblebee-nameplate--motion-scan")).toBe(true);
    } finally {
      dom.restore();
    }
  });

  test("sanitizes class tokens without hard-coded style allowlists", async () => {
    const dom = installDocumentHarness();
    try {
      const { Nameplates } = await import(`./nameplates.ts?test=${Date.now()}`);
      const nameplates = new Nameplates();

      nameplates.show(
        "future-style",
        {
          text: "Alice",
          style: {
            background: "#ffffff",
            border: "#111111",
            text: "#222222",
            accent: "#ffcc00",
            shadow: "rgba(0, 0, 0, 0.25)",
            fontFamily: "futureFont" as never,
            shape: "futureShape" as never,
            decoration: "futureDecor" as never,
            motion: "futureMotion" as never,
          },
        },
        { x: 120, y: 90, visible: true },
      );

      const futureElement = dom.body.children[0];
      expect(hasClass(futureElement!, "bumblebee-nameplate--futureShape")).toBe(
        true,
      );
      expect(
        hasClass(futureElement!, "bumblebee-nameplate--font-futureFont"),
      ).toBe(true);
      expect(
        hasClass(futureElement!, "bumblebee-nameplate--decoration-futureDecor"),
      ).toBe(true);
      expect(
        hasClass(futureElement!, "bumblebee-nameplate--motion-futureMotion"),
      ).toBe(true);

      nameplates.show(
        "unsafe-style",
        {
          text: "Bob",
          style: {
            background: "#ffffff",
            border: "#111111",
            text: "#222222",
            accent: "#ffcc00",
            shadow: "rgba(0, 0, 0, 0.25)",
            fontFamily: "bad token" as never,
            shape: "bad}token" as never,
            decoration: "bad/token" as never,
            motion: "bad token" as never,
          },
        },
        { x: 140, y: 110, visible: true },
      );

      const unsafeElement = dom.body.children[1];
      expect(hasClass(unsafeElement!, "bumblebee-nameplate--capsule")).toBe(
        true,
      );
      expect(
        hasClass(unsafeElement!, "bumblebee-nameplate--font-rounded"),
      ).toBe(true);
      expect(
        hasClass(unsafeElement!, "bumblebee-nameplate--decoration-shine"),
      ).toBe(true);
      expect(unsafeElement!.className).not.toContain("bad");
    } finally {
      dom.restore();
    }
  });
});
