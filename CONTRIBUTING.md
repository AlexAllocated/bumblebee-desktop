# Contributing

Bumblebee Desktop is a new Rust/Tauri application with a Svelte interface. The private historical SaaS repository is a reference, not a dependency. Project documentation and GitHub issues must remain useful without Linear.

Start with `AGENTS.md` and `docs/architecture.md`. Keep provider credentials in the operating-system keyring; never include local configuration, personal transcripts, generated media or production exports in commits. Artwork has separate terms in `ASSETS.md`.

Use focused changes and regression tests for conversation state, permissions, image approval, audio interruption and restart recovery. Run the relevant Rust and Svelte checks, and describe which installer or live-provider boundaries were actually exercised. Never claim a queued request or missing provider response proves success.

See `docs/verification.md` for the current acceptance evidence. Installer builds are unsigned previews until signing is explicitly adopted. No hosted deployment or paid signing service is required.
