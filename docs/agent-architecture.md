# Native agent behavior

`crates/core/src/agent` is the Rust turn loop shared by stream chat, Discord text and Discord voice. It does not run Nucleus or depend on a hosted account. Provider credentials stay in the OS credential store; turn state, pending answers, action receipts, memories and reminders stay in SQLite.

## A request and its effects

Every request starts by configuring immutable delivery: its source, an explicitly selected Discord DM, speech, and public progress. Private Discord sources and private-only delivery suppress speech and public progress. Additional destinations require an explicit delivery tool. Cross-account and cross-channel delivery requires the verified streamer identity and the relevant provider permissions. Local previews use the trusted desktop actor.

The model receives a closed function catalog and a strict final response schema. Arguments are checked against those schemas before an executor sees them. Display names, messages, saved history, memories, web pages and tool results are data rather than instructions. Native executors recheck live ownership, explicit Settings grants, OAuth scopes and Discord permissions; discovering a tool does not grant it.

Chat and voice use the same Responses API loop. The first request forces `configureTurnDelivery`; later requests retain the tool catalog, original input, complete function calls and their ordered outputs, and encrypted reasoning items. `store: false` keeps continuation in the local checkpoint instead of relying on a provider-side response ID. An executor failure returns an explicit failed result to the model; independent later calls can finish. A pending question or confirmation suspends the remaining batch. A progress update or queued operation never completes the parent turn.

Terminal output is a buffered assistant message with exactly `text` and `messages`, never a synthetic final tool. The runtime validates the same closed schema locally before any delivery, including required nullable fields. Semantic message groups retain their explicit artifact IDs and their text must join to the fallback text. Recognizable nested legacy delivery envelopes are rejected; ordinary requested JSON and fenced code remain content. A valid empty reply stays empty when a previous `deliverMessage` already fulfilled delivery. If the terminal envelope is malformed after delivery was configured, the runtime sends one fixed formatting-failure reply through that saved policy and marks the turn failed; it never publishes malformed protocol text or records that fallback as a successful answer.

Public progress and nonverbal thinking are separate. Chat progress is text. A voice progress update is spoken only when the turn allows public progress and speech and that update explicitly sets `spoken: true`; otherwise it is suppressed in the call ledger. Private/silent routes cannot speak progress. Ordinary spoken voice finals are not duplicated into the associated text channel; explicitly grouped files still go there, and explicit `deliverMessage` calls retain their own destination and receipt.

Calls execute in provider order, with at most 24 model rounds and 96 calls per turn. Saved context is bounded to 8 MiB. The SQLite ledger records a call before dispatch and retains its observed result. Reusing a call ID with different arguments fails. A missing response, cancellation after dispatch, or an ambiguous provider acknowledgment leaves the action unknown. Equivalent unknown actions remain blocked even under a different call ID or turn; known resource mutations use exact target identity so changing an audit reason cannot bypass that guard. A closed list of audited read-only tools may be deliberately invoked again under a new call ID, so an interrupted inspection cannot permanently disable discovery. Local memory, reminder and settings mutations are not classified as reads. There are no automatic mutation retries.

Destructive calls suspend before dispatch with their exact saved arguments. The confirmation belongs to its original requester and turn, has an expiry, and is consumed once in the same transaction that resumes the saved checkpoint. Approval rechecks current permission. Declining skips that action and preserves later calls. Cancellation ends the remaining work. `requestUserInput` uses the same durable continuation rather than discarding the request at a question.

Repairable missing credentials, OAuth scopes, enabled tool groups or observed Discord permission bits pause the exact unexecuted call before its ledger entry. The dashboard offers `continue` or `cancel`; changing Settings does not itself continue the action. Continue rechecks access against the same saved call and provider identity. A formerly advertised tool that has since been disabled can reach this setup barrier through the closed executor catalog. Owner-only calls denied to the requester never run privileged access discovery. A target mismatch or ordinary provider rejection is not treated as a reconnect prompt, and an explicit decline does not require restoring the declined action's access.

The saved reply route is separate from final delivery. Private questions go to the Discord requester, or to the explicitly configured Discord owner for a currently verified Twitch/YouTube streamer. The original platform identity and saved Discord identity are both checked again when continuing. A third-party final recipient never gains approval authority. Silent delivery and unavailable private routes leave the question in the dashboard without a public fallback. Trusted dashboard answers resume the original actor's turn and cancellation scope without entering the overlay or ordinary chat readout. The route persists in the checkpoint across restarts; there is no inferred cross-platform account linking.

On restart, pending questions can still be answered. The session also makes one bounded recovery attempt per process for up to 64 recent crash-interrupted turns, waiting up to 30 seconds for their original connections. Recovery uses the normal actor-owned queue and the saved model/tool budgets. Completed calls contribute their saved results; in-flight calls contribute an explicit unknown outcome. Neither is dispatched again. Safe remaining calls and model rounds can continue, and a validated terminal reply saved before delivery can finish without another model request. If setup is still unavailable, the turn stays interrupted for an explicit Resume after repair.

Only turns created with the positive recovery marker in this version are eligible, and only for 24 hours. Older interrupted records cannot distinguish a crash from an earlier explicit cancellation, so upgrades leave those records for inspection. Explicit stop/cancel is terminal for current and staged work, including crash work not yet queued. Interrupted final delivery remains unknown and never resumes. Acknowledging an interrupted turn hides it without erasing uncertain calls. There is no automatic mutation replay or blanket clearing of unknown effects.

Saved requests retain verified original owner status and nonsecret source-account bindings. Resumption and every actual model/tool/final boundary recheck those bindings. Changing the Discord bot, owner, guild or source channel, replacing a Twitch/Google account, or revoking the requester's current access cannot transfer a saved conversation to the new identity. Credentials can refresh or rotate for the same account. Provider event badges are not durable authority: recovered viewer requests that depended on old role badges require fresh permission evidence. Old saved requests without a provider identity cannot silently acquire the current one.

Returned partial message failures preserve acknowledged message IDs, completed/total parts and whether the unacknowledged part may have sent. Later parts are not sent after a failure. The saved final checkpoint also retains completed earlier destinations, and tool failures keep their receipts in the call ledger. `Store::interruption_receipts` projects only bounded status, destination, IDs and counts for trusted desktop inspection; it never returns request text, arguments, raw provider errors or secrets. Abrupt cancellation or process failure can still lose acknowledgments held only in a sending future: this is not a per-part durable journal or an exactly-once transport guarantee. Such outcomes remain unknown and cannot replay. Unknown tool rows survive even if their parent finishes or is dismissed; there is no general force-retry override.

Voice requests revalidate their listener before each model request, tool execution and final delivery. A saved request also remains bound to its original voice channel; moving the configured connection cannot silently move a pending conversation.

Disabling the agent blocks new voice captures, transcription dispatch and pending-answer continuation, including work that was queued before the setting changed. Pending requests remain saved; enable the agent before answering them, or cancel them while it is disabled. Ordinary chat and puppet commands do not resume pending agent work while disabled. Local stop/cancel keywords remain available. This preview requires the configured wake phrase for each spoken reply and does not open a follow-up listening window after questions. Spoken prompts explain that requirement; exact voice confirmations and fixed choices tolerate the wake address and sentence-ending periods/exclamation marks, with literal choice labels taking precedence. Compound or ambiguous statements never become approval.

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

Those last two restrictions and wake-required spoken replies are remaining feature differences from the retired application, not evidence of missing multi-round orchestration. The retired application could resolve attached/replied image originals, attach selected files to additional permitted Discord channels, and open a bounded follow-up listening window. The desktop does not claim those behaviors yet. Neither implementation maintains a deterministic ledger of every semantic obligation in a user's prose; the model summarizes observed tool results. There is no separate hidden spoken-summary field: explicit deliveries can carry long details and the terminal text remains the spoken summary/fallback.

Reminders are delivered only while the app is running. A due reminder is claimed atomically before dispatch. Restart or interruption during its delivery marks it unknown rather than sending it twice.

## Other local data guarantees

Chatter image approval is tied to exact normalized saved bytes. Request tickets are issued before download so an older slow response cannot replace a newer request, reset or block. Pending and rejected images are not OBS assets. A viewer block does not prevent the streamer from moderating that profile.

Speech audio and word timing occupy one complete, checksummed cache row. Hits slide expiry by three days. Partial or corrupt entries miss; a configured byte budget and entry limit evict the least recently used complete entries. No marketing prewarming occurs.

SQLite migrations are versioned and transactional. An unknown newer schema is refused without downgrading it.

## Evidence and remaining verification

`cargo test -p bumblebee-core --lib` exercises the actual shared loop using controlled model/provider boundaries: ordered multi-step failures and successful continuation, restart-safe confirmations, exact access-repair continuations, changed owner/account bindings, rejection, cancellation after local and remote dispatch, new-call-ID uncertainty guards, strict terminal output and safe formatting failure, private history and reminder interruption. Reopened-database recovery tests cover saved successful and unknown calls, remaining batch order/budgets, queued cancellation, ambiguous pre-upgrade checkpoints and the terminal delivery fence. Local HTTP tests cover first-part rejection, later-part rejection, unreadable acknowledgments and no subsequent send; receipt projections are tested for private-field exclusion. Storage tests cover migration preservation, request ordering, image approval visibility and cache corruption/expiry/eviction.

These tests do not establish live OAuth permission behavior, live Discord/Twitch/YouTube mutations, live OpenAI research/image/code execution, real-user reminder delivery, or packaged OBS/audio behavior. Those require separately observed integration checks with the user's configured providers. Unknown provider outcomes require inspection; the application does not offer a general force-retry override.

## Opt-in live Responses probe

The `agent_probe` example verifies the actual Responses request, full native function catalog, strict final parser, shared turn loop and durable SQLite result against a selected model. It uses an isolated temporary database and an OpenAI-only credential adapter. Its delivery boundary accepts only a silent local source and stores the final greeting locally; every other tool executor, audio delivery, external destination and confirmation prompt is rejected. It never starts the application engine or platform connections. The example also reopens SQLite to verify persistence, then removes its temporary data.

Load the saved OpenAI key into the child process environment as `BUMBLEBEE_AGENT_PROBE_KEY`, without printing it or placing it in shell arguments. Then run explicitly:

```sh
cargo run -p bumblebee-core --features live-probes --example agent_probe -- gpt-6-astra
```

The model argument is required, and availability is checked with the same credential before inference; use a model enabled for that API project. The current [OpenAI model catalog](https://developers.openai.com/api/docs/models) describes supported model capabilities. This probe does not set a model in application Settings. It permits at most three model requests and a 90-second turn; it is not run in CI and is absent from normal builds unless the `live-probes` feature is enabled.
