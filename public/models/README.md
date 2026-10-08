# Runtime Model Assets

These model assets are packaged with the desktop application and served through its token-protected OBS endpoint.

- `bumblebee.cb67e11b.glb` is the content-addressed optimized runtime model loaded by the Bumblebee overlay package. It
  retains the 16 animation clips exposed by the package, resamples redundant baked keyframes at
  a 0.00001 tolerance, and stores its display-sized 2048×2048 texture as lossless WebP. Keep the
  full 4096×4096 authoring export outside `public/`.
- `dandelion.216acf70.glb` is a content-addressed optimized runtime copy preserved with the original artwork.

Do not store source archives, marketplace downloads, or authoring exports in `public/`.
Model source, license, and conversion notes belong in the asset notices and attribution records before a runtime file is added here.
