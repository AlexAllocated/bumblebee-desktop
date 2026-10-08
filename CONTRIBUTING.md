# Contributing

Bumblebee Desktop is a new Rust/Tauri application with a Svelte interface. The private historical SaaS repository is a reference, not a dependency. Project documentation and GitHub issues must remain useful without Linear.

Start with `AGENTS.md` and `docs/architecture.md`. Keep provider credentials in the operating-system keyring; never include local configuration, personal transcripts, generated media or production exports in commits. Artwork has separate terms in `ASSETS.md`.

Use focused changes and regression tests for conversation state, permissions, image approval, audio interruption and restart recovery. Run the relevant Rust and Svelte checks, and describe which installer or live-provider boundaries were actually exercised. Never claim a queued request or missing provider response proves success.

This is one monorepo: Rust crates live in `crates/`, the Tauri shell in `src-tauri/`, the interface in `src/`, and the reusable `@hivetech/bumblebee` and `@hivetech/speech-bubbles` npm packages in `packages/`. The desktop resolves those packages from the workspace. Put shared rendering changes and their tests in the packages rather than copying implementations into the app.

Run `bun install --frozen-lockfile`, `bun run check:packages`, `bun run check`, and `bun run test` from the repository root. `bun run build:packages` builds both npm distributions; `bun run build` builds the desktop and OBS assets. Native checks use `cargo test --workspace --locked` inside the development environment described in [packaging](docs/packaging.md). Publishing an npm package is a separate, deliberate release step.

See `docs/verification.md` for the current acceptance evidence. Installer builds are unsigned previews until signing is explicitly adopted. No hosted deployment or paid signing service is required.
