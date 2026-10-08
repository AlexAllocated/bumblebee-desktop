# Bumblebee Desktop

This is the new standalone desktop product, not the archived SaaS stack.

- Rust owns application behavior, providers, credentials, SQLite and audio. Svelte owns the desktop interface. The localhost server exposes OBS rendering only.
- Preserve cancellation, confirmation identity and uncertain-action recovery. Never repeat an external action merely because its previous result is missing.
- No Docker, server databases, bundled JavaScript backend, cloud storage, accounts, billing, promotions, mods or raffles.
- Keep secrets out of Git and browser assets. Store keys in the OS credential store.
- Keep the repository self-contained; Linear is not a required source of project knowledge.
- Run focused Rust and frontend tests, then validate packaged Windows/Linux behavior. A build is not proof of live provider or OBS/audio behavior.
- Do not claim an unfinished capability works. Record remaining verification in docs/verification.md.
