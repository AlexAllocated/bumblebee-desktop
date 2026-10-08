# Native agent behavior

`crates/core/src/agent` is the Rust turn loop shared by stream chat, Discord text and Discord voice. It does not run Nucleus or depend on a hosted account. Provider credentials stay in the OS credential store; turn state, pending answers, action receipts, memories and reminders stay in SQLite.

## A request and its effects

Every request starts by configuring immutable delivery: its source, an explicitly selected Discord DM, speech, and public progress. Private Discord sources and private-only delivery suppress speech and public progress. Additional destinations require an explicit delivery tool. Cross-account and cross-channel delivery requires the verified streamer identity and the relevant provider permissions. Local previews use the trusted desktop actor.

The model receives a closed function catalog and a strict final response schema. Arguments are checked against those schemas before an executor sees them. Display names, messages, saved history, memories, web pages and tool results are data rather than instructions. Native executors recheck live ownership, explicit Settings grants, OAuth scopes and Discord permissions; discovering a tool does not grant it.

Calls execute in provider order, with at most 24 model rounds and 96 calls per turn. Saved context is bounded to 8 MiB. The SQLite ledger records a call before dispatch and retains its observed result. Reusing a call ID with different arguments fails. A missing response, cancellation after dispatch, or an ambiguous provider acknowledgment leaves the action unknown. Equivalent unknown actions remain blocked even under a different call ID or turn; known resource mutations use exact target identity so changing an audit reason cannot bypass that guard. There are no automatic mutation retries.

Destructive calls suspend before dispatch with their exact saved arguments. The confirmation belongs to its requester, actual reply channel and turn, has an expiry, and is consumed once in the same transaction that resumes the saved checkpoint. Approval rechecks current permission. Declining skips that action and preserves later calls. Cancellation ends the remaining work. `requestUserInput` uses the same durable continuation rather than discarding the request at a question.

On restart, pending questions can still be answered. In-flight tool effects and interrupted final delivery become unknown; other interrupted turns are surfaced for inspection. Acknowledging an interrupted turn hides it from the dashboard without erasing its uncertain-action ledger. There is deliberately no automatic replay or blanket clearing of unknown effects.

Voice requests revalidate their listener before each model request, tool execution and final delivery. A saved request also remains bound to its original voice channel; moving the configured connection cannot silently move a pending conversation.

## Retained capabilities

The executable catalog includes:

- Delivery, progress, durable questions and exact destructive confirmations.
- Scoped memories, scoped conversation history and durable Discord DM reminders.
- Application, voice, overlay and chatter settings, plus conflict-checked undo of local settings changes.
- Native Discord resource discovery, channel/role/message/member operations and moderation; Twitch discovery, stream management, moderation and polls; YouTube discovery, moderation and polls. See [platform tools](platform-tools.md) for current permissions and provider boundaries.
- Real provider-managed web research with citations, code execution with downloaded file results, image generation and image editing from exact saved artifacts.
- Explicit artifact attachments, private local artifact storage, and owner-selected image/text presentation on stream.
- Voice replay saved from the native audio buffer when replay is explicitly enabled.

Generated work returns to the parent turn for further tools or delivery. It is not automatically displayed on stream. Twitch and YouTube chat cannot receive files. Requester/source Discord delivery supports attachments; arbitrary additional Discord channels currently accept text. Image edits accept saved generated artifact IDs; arbitrary remote image attachments are not part of the current agent input contract.

Reminders are delivered only while the app is running. A due reminder is claimed atomically before dispatch. Restart or interruption during its delivery marks it unknown rather than sending it twice.

## Other local data guarantees

Chatter image approval is tied to exact normalized saved bytes. Request tickets are issued before download so an older slow response cannot replace a newer request, reset or block. Pending and rejected images are not OBS assets. A viewer block does not prevent the streamer from moderating that profile.

Speech audio and word timing occupy one complete, checksummed cache row. Hits slide expiry by three days. Partial or corrupt entries miss; a configured byte budget and entry limit evict the least recently used complete entries. No marketing prewarming occurs.

SQLite migrations are versioned and transactional. An unknown newer schema is refused without downgrading it.

## Evidence and remaining verification

`cargo test -p bumblebee-core --lib` exercises the actual shared loop using controlled model/provider boundaries: ordered multi-step calls, restart-safe confirmations, changed permissions, rejection, cancellation after dispatch, new-call-ID uncertainty guards, malformed terminal output, private history and reminder interruption. Local HTTP tests cover provider rejection versus ambiguous acknowledgment, truncation and cancellation after receipt. Storage tests cover migration preservation, request ordering, image approval visibility and cache corruption/expiry/eviction.

These tests do not establish live OAuth permission behavior, live Discord/Twitch/YouTube mutations, live OpenAI research/image/code execution, real-user reminder delivery, or packaged OBS/audio behavior. Those require separately observed integration checks with the user's configured providers. Unknown provider outcomes require inspection; the application does not offer a general force-retry override.

## Opt-in live Responses probe

The `agent_probe` example verifies the actual Responses request, full native function catalog, strict final parser, shared turn loop and durable SQLite result against a selected model. It uses an isolated temporary database and an OpenAI-only credential adapter. Its delivery boundary accepts only a silent local source and stores the final greeting locally; every other tool executor, audio delivery, external destination and confirmation prompt is rejected. It never starts the application engine or platform connections. The example also reopens SQLite to verify persistence, then removes its temporary data.

Load the saved OpenAI key into the child process environment as `BUMBLEBEE_AGENT_PROBE_KEY`, without printing it or placing it in shell arguments. Then run explicitly:

```sh
cargo run -p bumblebee-core --features live-probes --example agent_probe -- gpt-5.4-mini
```

The model argument is required, and availability is checked with the same credential before inference. The current [GPT-5.4 Mini documentation](https://developers.openai.com/api/docs/models/gpt-5.4-mini) lists Responses, function calling and structured output support. This probe does not set a model in application Settings. It permits at most three model requests and a 90-second turn; it is not run in CI and is absent from normal builds unless the `live-probes` feature is enabled.
