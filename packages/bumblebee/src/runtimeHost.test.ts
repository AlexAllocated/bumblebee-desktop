import { describe, expect, test } from "bun:test";
import { createRuntimeHost } from "./runtimeHost";
describe("asset runtime host", () => {
  test("preserves an explicit capability directory for all packaged assets", () => {
    const host = createRuntimeHost({
      assetBaseUrl: "http://127.0.0.1:2899/capability/",
    });
    expect(host.httpUrl("/models/bumblebee.glb")).toBe(
      "http://127.0.0.1:2899/capability/models/bumblebee.glb",
    );
    expect(host.httpUrl("puppets/images/dandy.png")).toBe(
      "http://127.0.0.1:2899/capability/puppets/images/dandy.png",
    );
    expect("authHeaders" in host).toBe(false);
  });
  test("requires a host outside a document instead of contacting a hosted service", () => {
    expect(() => createRuntimeHost()).toThrow("assetBaseUrl");
  });
});
