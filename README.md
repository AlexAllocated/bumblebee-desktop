# Bumblebee Desktop

Bumblebee is a streaming companion with a voice, a personality, and a puppet for every chatter. This is the standalone desktop successor to Bumblebee's hosted service: a Rust application with a Svelte interface, packaged with Tauri for Windows and Linux.

**Development preview:** the desktop rewrite is being integrated and tested. See [verification status](docs/verification.md) for what has actually passed. An installer build is not evidence of a complete streaming session.

## Using Bumblebee

The intended installation flow is:

1. Download an installer from [Releases](https://github.com/AlexAllocated/bumblebee-desktop/releases). Preview installers are unsigned; compare their SHA-256 checksum with the release's checksum file.
2. Open Settings and configure only the providers you want to use. Azure Speech supplies voices, OpenAI supplies the agent and transcription, and Twitch, YouTube, and Discord connect your communities. Each integration uses your own credentials or application registration.
3. Authorize Twitch or YouTube in your system browser. Discord requires your own bot application and bot token; Bumblebee does not automate a personal Discord account.
4. Add the local overlay URL shown in Bumblebee to an OBS browser source, then start a session.

Provider services can charge for use. Bumblebee has no hosted account, subscription, or bundled provider credentials. You do not need Docker, a database server, or a JavaScript runtime to run a packaged installer. Closing the window during an active session keeps Bumblebee in the tray; Quit stops the session.

Follow the [provider setup guide](docs/provider-setup.md) for application registration, authorization, voice testing, and OBS setup.

## Chat puppets

A viewer receives a random built-in puppet and voice on first participation. That selection persists across restarts and is tied to the platform's stable user ID.

| Command | Action |
| --- | --- |
| `!puppet` | Show the current selection and help |
| `!puppet random` | Pick another built-in puppet |
| `!puppet <name>` | Select a built-in puppet |
| `!puppet <https-image-url>` | Submit an image for streamer approval |
| `!voice` | Show the current voice and help |
| `!voice random` | Pick another voice |
| `!voice <name>` | Select a curated preset or an Azure English voice |
| `!voices <search>` | Find voices; use the returned pagination hint for more |

Settings contains image approvals and searchable chatter profiles. An approved image is stored locally: changing the original URL cannot replace what the streamer approved. Viewers can be reset or blocked from customization. Bumblebee's own curated voice is configured separately.

## Local data and permissions

SQLite stores settings, profiles, memories, reminders, pending questions, and the tool-action ledger. Credentials live in the OS credential store. Installed artwork is separate from generated files and approved viewer images in the application-data directory. Dynamic speech caching is bounded and expires after three days without use; there is no pregenerated speech cache.

The OBS endpoint listens on loopback, normally port 2899, and uses a persistent, revocable token. It serves rendering content, not application controls or credentials. Treat its URL as private. The agent's platform-management abilities are opt-in and require current provider permissions; consequential actions require confirmation. An interrupted action with an unknown result is not automatically repeated.

## Development and contribution

Read [CONTRIBUTING.md](CONTRIBUTING.md), [architecture](docs/architecture.md), and [packaging](docs/packaging.md). Installer workflows replace the old cloud deployment system. The repository is self-contained and does not require access to the old private project or Linear.

Project code is [MIT licensed](LICENSE). Artwork, TEN VAD, Azure Speech SDK, and other dependencies retain separate terms: read [asset notices](ASSETS.md) and [third-party notices](THIRD_PARTY_NOTICES.md). Artwork includes Alopex's work; its attribution must remain with redistributed packages.
