# Application architecture

Bumblebee is an installable streaming companion. Its Svelte desktop dashboard calls typed Tauri commands; a Rust core owns provider connections, chatter preferences, the agent turn loop and media. Integrated Songbird retains its audio scheduler. OBS connects to a token-scoped localhost renderer, never to the administration interface.

SQLite stores durable state, including pending agent work. A tool action is recorded before dispatch. Completed calls reuse observed results; interrupted calls become unknown and cannot automatically run again. Provider identity and current grants must be checked again when a confirmation resumes.

Puppet identity uses platform plus stable user ID. First assignments persist. Image submissions are downloaded and normalized once; the streamer approves immutable local bytes. Changing a URL remotely cannot change approved artwork.

Original art ships in Git and application resources. Installation-specific files live under the operating system's application-data directory. Credentials live in the operating system credential store. Speech is cached dynamically with bounded storage and a three-day sliding expiration; no marketing pregeneration exists.

The original private repository preserves SaaS history. The desktop repository has independent history and must not import private configuration or production user data. Code is MIT licensed; asset and dependency notices remain separate.

## Rendering packages and desktop adapter

The workspace retains `@hivetech/bumblebee` and `@hivetech/speech-bubbles` as independent packages under `packages/`. Bumblebee owns actor state machines, motion, camera-facing placement, speech playback, puppet lifecycle, nameplates and bubble coordination. Speech bubbles owns its framework-independent SVG renderer, geometry, pagination, timed reveal and frame rendering. Their reusable APIs and behavioral tests live with the packages; the desktop must not maintain a competing copy of those implementations.

The desktop `Stage` adapts typed Rust events and saved layout into those package APIs. Rust provides complete local speech audio and word timings; the package consumes those prepared assets without hosted authentication or a JavaScript provider server. Both the dashboard preview and OBS use that adapter. The editor adds desktop-only drag, resize, anchor, preview and reset controls. Its writes go through Tauri commands, never through the OBS HTTP endpoint.

The retained overlay subjects are Bumblebee, ordinary chat puppets and the standalone streamer caption bubble. Discord participant puppets and their settings have been removed. Streamer captions are separately opt-in, require the configured owner, and cancel pending transcription when disabled. Voice conversation still uses the native Discord audio runtime.

## Settings and recovery

The single Settings drawer saves sparse, validated changes. SQLite applies a patch atomically against the latest stored values. The UI serializes saves and preserves edits made while an earlier request is in flight, including failed drafts. Routine volume and layout changes update their live runtime behavior without reconnecting every platform; connection identity changes retire and rebuild the affected session safely.

Settings contains named Discord resource selection, per-platform readout/mention audiences, opt-in chat relay, audio routing and mixing, model/tool controls, per-viewer overrides, image approvals and memory/reminder management. Follower-only Twitch audiences require the additional provider scope and use a bounded lookup; unknown follower state grants no access. Local behavior resets preserve connection identities, credentials, user data and the OBS capability token. The schema migration from the first preview preserves installation settings, profiles and undo state while adapting the flattened layout.
