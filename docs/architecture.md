# Application architecture

Bumblebee is an installable streaming companion. Its Svelte desktop dashboard calls typed Tauri commands; a Rust core owns provider connections, chatter preferences, the agent turn loop and media. Integrated Songbird retains its audio scheduler. OBS connects to a token-scoped localhost renderer, never to the administration interface.

SQLite stores durable state, including pending agent work. A tool action is recorded before dispatch. Completed calls reuse observed results; interrupted calls become unknown and cannot automatically run again. Provider identity and current grants must be checked again when a confirmation resumes.

Puppet identity uses platform plus stable user ID. First assignments persist. Image submissions are downloaded and normalized once; the streamer approves immutable local bytes. Changing a URL remotely cannot change approved artwork.

Original art ships in Git and application resources. Installation-specific files live under the operating system's application-data directory. Credentials live in the operating system credential store. Speech is cached dynamically with bounded storage and a three-day sliding expiration; no marketing pregeneration exists.

The original private repository preserves SaaS history. The desktop repository has independent history and must not import private configuration or production user data. Code is MIT licensed; asset and dependency notices remain separate.
