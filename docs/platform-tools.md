# Provider actions

Provider tools run inside the Rust agent loop and use the streamer's configured
accounts. Discord REST calls use the same bot identity as the single native
gateway; they do not start another gateway. No tool accepts an arbitrary HTTP
endpoint, bearer token, guild switch, or SQL command.

Discovery returns exact provider IDs. Confirmed mutations retain those IDs:
Discord actions bind a guild, Twitch actions bind a broadcaster, and YouTube
actions bind a live-chat ID. Renaming a channel or changing Settings does not
retarget an approved action. Each execution checks the current human owner,
selected local tool group, and live provider identity/permissions. Discord checks
both the bot and configured human owner, including channel overwrites, private
thread membership, timeouts and role hierarchy. Twitch acts only on its authorized
broadcaster's own channel. YouTube validates ownership of an active broadcast.

Enabled tool groups are opt-in. Twitch authorization requests the base chat scopes
plus only the scopes needed by selected groups. Enabling another group can require
reconnecting Twitch. Existing tokens never acquire permissions just because a
checkbox changed.

| Group | Actions |
| --- | --- |
| `discord_resources` | Create/edit/delete channels, create public threads, delete/pin/unpin messages, send to a discovered channel |
| `discord_moderation` | Move/disconnect voice users, timeouts, bans, role assignment, nicknames, Stage speaker/audience/mute |
| `twitch_broadcast` | Stream title/category/tags, raids, commercials, markers and clips |
| `twitch_moderation` | Bans/timeouts, chat modes and shoutouts |
| `twitch_polls` | Create and close Twitch polls |
| `youtube_moderation` | Live-chat bans and exact-message deletion |
| `youtube_polls` | Create and close YouTube polls |

Cross-platform polls are separate tool calls so one successful action does not
cause another provider's failed or uncertain action to replay it. YouTube polls
have no automatic duration or channel-points voting. Closing a YouTube poll shows
its result; Twitch can terminate or archive it. For YouTube deletion the exact
message must still be discoverable in the recent chat response.

There are no automatic retries for writes. HTTP rejection is a known failure;
lost/interrupted responses, server failures, and unreadable or oversized successful
receipts are unknown outcomes. The durable tool ledger must retain that distinction
and stop the turn for human reconciliation. An acknowledged asynchronous clip is
reported as accepted, not as a finished video. Twitch metadata changes include
read-back verification. Discord and YouTube successful mutations report accepted
provider receipts, without claiming a later observed state.

The former `setTwitchGoLiveNotificationText` configured Bumblebee's automatic
announcement copy, not Twitch's native notification. It is excluded along with
automatic announcements, promotions and celebrations.

Tests cover permission precedence and hierarchy, explicit OAuth grants, exact-ID
validation, strict schemas, bounded responses, lost write acknowledgments,
cancellation after dispatch, and no implicit HTTP retries. These are local tests;
provider credentials, moderator permissions, actual API mutations and Discord
voice behavior require an explicit integration run before release acceptance.

API references used for the implementation:

- [Discord permissions](https://docs.discord.com/developers/topics/permissions)
- [Discord messages](https://docs.discord.com/developers/resources/message)
- [Discord channels](https://docs.discord.com/developers/resources/channel)
- [Discord guilds](https://docs.discord.com/developers/resources/guild)
- [Discord voice](https://docs.discord.com/developers/resources/voice)
- [Twitch API](https://dev.twitch.tv/docs/api/reference/)
- [YouTube broadcasts](https://developers.google.com/youtube/v3/live/docs/liveBroadcasts/list)
- [YouTube chat insertion and polls](https://developers.google.com/youtube/v3/live/docs/liveChatMessages/insert)
- [YouTube poll transition](https://developers.google.com/youtube/v3/live/docs/liveChatMessages/transition)
