# Connect your providers

Open **Settings → Connections**. Configure only the services you want: Azure Speech supplies voices, OpenAI supplies the agent, and the streaming platforms supply chat. Keys and authorization tokens are saved in your operating-system credential store. A saved key is not the same as a successful connection; use the validation buttons and check the dashboard's connection status.

## Speech and the agent

For Azure Speech, create or select a Speech resource in the Azure portal. Copy its key into **Speech key**, enter that resource's region, and select **Save key**, then **Validate & refresh voices**. The region must belong to the same resource as the key. The Voices section now includes the regional English catalog alongside the curated presets. Use the dashboard's **Say it** button for a short test; select **Unmute preview** to hear it locally. Provider usage is billed to your account.

For OpenAI, save your API key, enter a Responses API model available to your account, and select **Validate**. Validation checks access; a live turn also needs a model that supports Bumblebee's tools and structured response format. Enable the agent and choose any additional agent permission groups you want. Speech and chat puppet commands can be used with the agent disabled.

## Twitch

1. In the [Twitch developer console](https://dev.twitch.tv/console/apps), register your own application with **Public** client type. Twitch requires a verified account with two-factor authentication. If registration requires a redirect URL, use `http://localhost:3000`; Bumblebee's device authorization flow does not use it.
2. Copy the **Client ID** into Bumblebee and enter your channel login. No Twitch client secret is needed.
3. Select **Authorize Twitch**. Complete the device-code prompt in your system browser using the account that should post messages.
4. Return to Bumblebee and start the session. If you later enable more Twitch agent permissions, authorize again to grant those scopes.

Twitch messages use the authorized account. Reauthorize if the account is disconnected or authorization expires. See Twitch's [registration instructions](https://dev.twitch.tv/docs/authentication/register-app/) and [public device flow](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#device-code-grant-flow).

## YouTube

1. Select a Google Cloud project, enable **YouTube Data API v3**, and configure its OAuth consent screen. Add your Google account as a test user when the project is in Testing mode.
2. Create an OAuth client with the **Desktop app** type in [Google Cloud credentials](https://console.cloud.google.com/apis/credentials). Copy the desktop client ID and client secret into Bumblebee, then save the secret.
3. Select **Authorize YouTube** and choose the account/channel that owns your broadcast. The browser returns directly to this computer. Google documents this [desktop loopback authorization flow](https://developers.google.com/youtube/v3/guides/auth/installed-apps).
4. Start a broadcast with live chat enabled, then start the Bumblebee session. Leave **Live chat ID** blank to discover your single active broadcast, or enter a specific live chat ID if you have more than one. A video ID is not a live chat ID.

External Google projects in Testing mode issue refresh tokens that expire after seven days for YouTube access. Reauthorize when needed, or review Google's publishing and verification requirements for your own project. See [Google's token expiration rules](https://developers.google.com/identity/protocols/oauth2#expiration).

## Discord

1. Create your own bot application in the [Discord developer portal](https://discord.com/developers/applications). Enable **Server Members Intent** and **Message Content Intent** on its Bot page; access remains subject to Discord's [privileged-intent requirements](https://support-dev.discord.com/hc/en-us/articles/6207308062871-What-are-Privileged-Intents).
2. Install the bot in your server with **View Channels**, **Send Messages**, **Read Message History**, **Connect**, and **Speak** permissions. Add permissions for moderation or other optional tools only when using those abilities.
3. Save the bot token in Bumblebee. Enable Developer Mode in Discord and copy your server ID, your own user ID, and the text/voice channel IDs into Settings. Channel overrides must allow the bot to access those channels.
4. Select **Validate**, save Settings, and start the session. Discord chat and voice share this one bot connection. Bumblebee does not sign in as or automate your personal Discord account.

Enable Bumblebee's agent to use voice requests. Turning it off blocks new wake-triggered captures and transcription uploads; local stop/cancel detection still works. Pending requests stay saved, but you must enable the agent to answer them; cancellation remains available while disabled. In this preview, every spoken reply needs the configured wake phrase, including answers to confirmations: say **Hey Bumblebee**, then **yes** or **no** (or **Bumblebee** if you selected that shorter wake phrase). Bumblebee does not automatically keep listening after asking a question. You can also answer pending requests in Discord text or the dashboard.

## OBS and the first session

Copy the overlay URL from the dashboard into an OBS **Browser Source**, normally at 1920 × 1080. Keep Bumblebee running. The URL is private to this computer and serves only the overlay; no tunnel or router forwarding is needed. Preview speech is muted by default to avoid hearing both the dashboard and OBS.

Use the dashboard stop button to interrupt speech. Closing the window during an active session leaves Bumblebee in the tray; **Quit Bumblebee** stops it. To disconnect a provider, remove its saved credential in Settings; this stops the active session. The current preview's tested and untested boundaries are recorded in [verification status](verification.md).
