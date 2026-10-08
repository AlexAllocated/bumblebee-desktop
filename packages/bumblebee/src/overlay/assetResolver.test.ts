import { describe, expect, test } from "bun:test";
import { createRuntimeHost } from "../runtimeHost";
import { AssetResolver } from "./assetResolver";
describe("AssetResolver", () => {
  const host = () =>
    createRuntimeHost({ assetBaseUrl: "http://127.0.0.1:2899/token/" });
  test("uses explicit model and approved image overrides", () => {
    const assets = new AssetResolver(host(), {
      bumblebeeModelUrl: "https://example.test/bee.glb",
      puppetImageUrl: (id) => `https://example.test/${id}.png`,
    });
    expect(assets.bumblebeeModel()).toBe("https://example.test/bee.glb");
    expect(assets.puppetImage("dandy")).toBe("https://example.test/dandy.png");
  });
  test("keeps models, puppets and sounds within the local asset directory", () => {
    const assets = new AssetResolver(host());
    expect(assets.bumblebeeModel()).toBe(
      "http://127.0.0.1:2899/token/models/bumblebee.cb67e11b.glb",
    );
    expect(assets.puppetImage("dandy")).toBe(
      "http://127.0.0.1:2899/token/puppets/images/dandy.png",
    );
    expect(assets.soundEffect("transitionWhoosh")).toEqual([
      "http://127.0.0.1:2899/token/audio/caption-morph-whoosh.mp3",
    ]);
  });
});
