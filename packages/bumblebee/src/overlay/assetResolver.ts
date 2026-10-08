import type { BumblebeeRuntimeHost } from "../runtimeHost";
import { soundEffectPaths } from "./sounds";
import type { OverlayAssetOptions, SoundEffect } from "./types";
export class AssetResolver {
  constructor(
    readonly host: BumblebeeRuntimeHost,
    readonly options?: OverlayAssetOptions,
  ) {}
  bumblebeeModel() {
    return (
      this.options?.bumblebeeModelUrl?.toString() ??
      this.host.httpUrl("models/bumblebee.cb67e11b.glb")
    );
  }
  puppetImage(id: string) {
    return (
      this.options?.puppetImageUrl?.(id) ??
      this.host.httpUrl(`puppets/images/${encodeURIComponent(id)}.png`)
    );
  }
  soundEffect(effect: SoundEffect) {
    const paths =
      this.options?.soundEffectUrls?.[effect] ?? soundEffectPaths[effect];
    return (Array.isArray(paths) ? paths : [paths]).map((path) =>
      this.host.httpUrl(path),
    );
  }
}
