# Third-party notices

The root MIT license covers Bumblebee project code. It does not replace dependency or asset licenses. Dependency versions are recorded in `Cargo.lock` and `bun.lock`; this document is not a declaration that every dependency uses MIT.

- **Songbird:** the vendored Rust Discord voice implementation and Bumblebee's audio-clock changes are under `vendor/songbird`. Preserve its license and attribution files with source distributions.
- **TEN VAD:** retained under TEN-framework's upstream license, including the additional conditions relating to Agora offerings and application use. It is not Apache-2.0 without additional terms, and Bumblebee does not relicense it as MIT. The native preparation script pins the source/artifacts and preserves their license alongside the runtime.
- **Microsoft Azure Speech SDK:** separately licensed Microsoft runtime components. See `crates/audio/native/azure-LICENSE.md`, `azure-REDIST.txt`, and `azure-ThirdPartyNotices.md`. Speech service access requires the user's own credentials.
- **Microsoft Visual C++ runtime:** Windows packages include the release x64 redistributables from the Visual Studio 2022 build installation, with the runtime license in `crates/audio/native/msvc-RUNTIME-LICENSE.txt`. Their version is recorded in the packaged native manifest/origin file. Redistribution is governed by Microsoft's [Visual Studio distributable-code terms](https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution), not Bumblebee's MIT license.
- **Linux runtime libraries:** Debian/Ubuntu release builds collect the installed GTK, WebKit, GStreamer and native-audio dependency notices, source-package names and exact versions into `licenses/third-party-licenses.txt`. These distribution packages retain their individual licenses; native audio dependency origins are recorded beside the libraries. TEN's release copy receives an `$ORIGIN` loader path; its implementation remains upstream's pinned binary.
- **Opus:** the codec retains the notices in `crates/audio/native/opus-LICENSE` and its dependency source.
- **Tauri, Svelte, Babylon.js, XState, SQLite, and other Rust/JavaScript dependencies:** retain their upstream licenses and copyright notices. Build tools and application dependencies are resolved from the lockfiles.
- **Artwork, character models, fonts, and sounds:** see [ASSETS.md](ASSETS.md); these are not automatically covered by the project code license.

No provider keys, OAuth tokens, production data, or original private repository history belong in a source or binary release.
