use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Puppet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub voice_name: String,
    #[serde(default = "azure")]
    pub provider: String,
    pub name: Option<String>,
    #[serde(default = "one")]
    pub rate: String,
    #[serde(default = "one")]
    pub pitch: String,
    #[serde(default = "normal")]
    pub expression: String,
    #[serde(default = "puppet")]
    pub role: String,
}
fn azure() -> String {
    "azure_speech".into()
}
fn one() -> String {
    "1.0".into()
}
fn normal() -> String {
    "default".into()
}
fn puppet() -> String {
    "puppet".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chatter {
    pub platform: String,
    pub user_id: String,
    pub display_name: String,
    pub puppet_id: String,
    pub voice_id: String,
    pub image_hash: Option<String>,
    pub customization_blocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSubmission {
    pub id: String,
    pub platform: String,
    pub user_id: String,
    pub display_name: String,
    pub image_hash: String,
    pub submitted_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub overlay_port: u16,
    pub azure_region: String,
    pub bumblebee_voice: String,
    pub openai_model: String,
    pub twitch_client_id: String,
    pub twitch_channel: String,
    pub google_client_id: String,
    pub youtube_live_chat_id: String,
    pub discord_guild_id: String,
    pub discord_text_channel_id: String,
    pub discord_voice_channel_id: String,
    pub owner_discord_id: String,
    pub enabled_tool_groups: Vec<String>,
    pub discord_listen_everyone: bool,
    pub discord_listen_role_ids: Vec<String>,
    pub discord_listen_allowed_user_ids: Vec<String>,
    pub discord_listen_blocked_user_ids: Vec<String>,
    pub wake_word: String,
    pub replay_enabled: bool,
    pub replay_seconds: u32,
    pub read_chat: bool,
    pub ai_enabled: bool,
    pub custom_images_enabled: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            overlay_port: 2899,
            azure_region: "eastus".into(),
            bumblebee_voice: "bumblebee-buddy".into(),
            openai_model: String::new(),
            twitch_client_id: String::new(),
            twitch_channel: String::new(),
            google_client_id: String::new(),
            youtube_live_chat_id: String::new(),
            discord_guild_id: String::new(),
            discord_text_channel_id: String::new(),
            discord_voice_channel_id: String::new(),
            owner_discord_id: String::new(),
            enabled_tool_groups: Vec::new(),
            discord_listen_everyone: false,
            discord_listen_role_ids: Vec::new(),
            discord_listen_allowed_user_ids: Vec::new(),
            discord_listen_blocked_user_ids: Vec::new(),
            wake_word: "hey_bumblebee".into(),
            replay_enabled: false,
            replay_seconds: 30,
            read_chat: true,
            ai_enabled: false,
            custom_images_enabled: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::ensure;
        ensure!(
            self.overlay_port >= 1024,
            "OBS port must be between 1024 and 65535"
        );
        ensure!(
            !self.azure_region.is_empty()
                && self.azure_region.len() < 40
                && self
                    .azure_region
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()),
            "Invalid Azure Speech region"
        );
        ensure!(
            crate::catalog::voices()
                .iter()
                .any(|v| v.id == self.bumblebee_voice && v.role == "bumblebee"),
            "Select a built-in Bumblebee voice"
        );
        ensure!(
            matches!(self.wake_word.as_str(), "bumblebee" | "hey_bumblebee"),
            "Unknown wake word"
        );
        ensure!(
            (5..=120).contains(&self.replay_seconds),
            "Replay buffer must be 5-120 seconds"
        );
        let groups = [
            "discord_resources",
            "discord_moderation",
            "twitch_broadcast",
            "twitch_moderation",
            "twitch_polls",
            "youtube_moderation",
            "youtube_polls",
        ];
        ensure!(
            self.enabled_tool_groups
                .iter()
                .all(|group| groups.contains(&group.as_str())),
            "Unknown tool permission group"
        );
        fn discord_id(value: &str) -> bool {
            value.parse::<u64>().is_ok_and(|id| id > 0)
        }
        for value in [
            &self.discord_guild_id,
            &self.discord_text_channel_id,
            &self.discord_voice_channel_id,
            &self.owner_discord_id,
        ] {
            ensure!(
                value.is_empty() || discord_id(value),
                "Discord IDs must be positive numeric IDs"
            );
        }
        for values in [
            &self.discord_listen_role_ids,
            &self.discord_listen_allowed_user_ids,
            &self.discord_listen_blocked_user_ids,
        ] {
            ensure!(
                values.len() <= 200 && values.iter().all(|v| discord_id(v)),
                "Discord permission lists support up to 200 numeric IDs"
            );
        }
        for value in [
            &self.openai_model,
            &self.twitch_client_id,
            &self.twitch_channel,
            &self.google_client_id,
            &self.youtube_live_chat_id,
        ] {
            ensure!(
                value.len() <= 512 && !value.chars().any(char::is_control),
                "A provider setting is too long or contains control characters"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OverlayEvent {
    OverlaySettings {
        settings: OverlaySettings,
    },
    Image {
        id: String,
        image_path: String,
        title: String,
    },
    HideImage,
    Presentation {
        title: String,
        text: String,
    },
    ChatterChanged {
        chatter: Chatter,
    },
    Chat {
        chatter: Chatter,
        text: String,
    },
    Speech {
        id: String,
        chatter: Option<Chatter>,
        text: String,
        audio_path: String,
        words: Vec<WordTiming>,
    },
    StopSpeech,
    Status {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct OverlaySettings {
    pub bee_x: f64,
    pub bee_y: f64,
    pub bee_scale: f64,
    pub bee_visible: bool,
    pub puppet_scale: f64,
    pub puppet_horizontal: f64,
    pub puppet_occlusion: f64,
    pub puppets_visible: bool,
    pub bubbles_visible: bool,
}
impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            bee_x: 0.15,
            bee_y: 0.82,
            bee_scale: 0.3,
            bee_visible: true,
            puppet_scale: 0.3,
            puppet_horizontal: 0.7,
            puppet_occlusion: 0.15,
            puppets_visible: true,
            bubbles_visible: true,
        }
    }
}
impl OverlaySettings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            [self.bee_x, self.bee_y, self.puppet_horizontal]
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "Overlay positions must be between 0 and 1"
        );
        anyhow::ensure!(
            [self.bee_scale, self.puppet_scale]
                .iter()
                .all(|v| v.is_finite() && (0.05..=0.8).contains(v)),
            "Overlay scales must be between 0.05 and 0.8"
        );
        anyhow::ensure!(
            self.puppet_occlusion.is_finite() && (0.1..=1.0).contains(&self.puppet_occlusion),
            "Puppet occlusion must be between 0.1 and 1"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordTiming {
    pub text: String,
    pub start_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub platform: String,
    pub user_id: String,
    pub display_name: String,
    pub message_id: String,
    pub channel_id: String,
    pub text: String,
    pub is_owner: bool,
}
