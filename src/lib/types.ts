export interface Settings {
  overlayPort: number;
  azureRegion: string;
  bumblebeeVoice: string;
  openaiModel: string;
  twitchClientId: string;
  twitchChannel: string;
  googleClientId: string;
  youtubeLiveChatId: string;
  discordGuildId: string;
  discordTextChannelId: string;
  discordVoiceChannelId: string;
  ownerDiscordId: string;
  readChat: boolean;
  aiEnabled: boolean;
  customImagesEnabled: boolean;
  enabledToolGroups: string[];
  discordListenEveryone: boolean;
  discordListenRoleIds: string[];
  discordListenAllowedUserIds: string[];
  discordListenBlockedUserIds: string[];
  wakeWord: string;
  replayEnabled: boolean;
  replaySeconds: number;
}
export interface Chatter {
  platform: string;
  userId: string;
  displayName: string;
  puppetId: string;
  voiceId: string;
  imageHash: string | null;
  customizationBlocked: boolean;
}
export interface Puppet {
  id: string;
  name: string;
  description: string;
}
export interface Voice {
  id: string;
  name: string | null;
  voiceName: string;
  provider: string;
  role: string;
  rate: string;
  pitch: string;
  expression: string;
}
export interface ImageSubmission {
  id: string;
  platform: string;
  userId: string;
  displayName: string;
  imageHash: string;
  submittedAt: number;
}
export interface ProviderStatus {
  provider: string;
  state: string;
  message: string;
}
export interface PendingInput {
  id: string;
  turnId: string;
  actor: string;
  channel: string;
  kind: string;
  prompt: string;
  choices: string[];
  ownerRequired: boolean;
  expiresAt: number;
}
export interface RecoveryTurn {
  id: string;
  actor: string;
  state: string;
  updatedAt: number;
}
export interface OverlaySettings {
  beeX: number;
  beeY: number;
  beeScale: number;
  beeVisible: boolean;
  puppetScale: number;
  puppetHorizontal: number;
  puppetOcclusion: number;
  puppetsVisible: boolean;
  bubblesVisible: boolean;
}
export interface Artifact {
  id: string;
  filename: string;
  mediaType: string;
  label: string;
}
export interface Snapshot {
  overlaySettings: OverlaySettings;
  settings: Settings;
  puppets: Puppet[];
  voices: Voice[];
  pendingImages: ImageSubmission[];
  chatters: Chatter[];
  overlayUrl: string;
  secrets: Record<string, boolean>;
  statuses: ProviderStatus[];
  active: boolean;
  credentialStoreError: string | null;
  overlayError: string | null;
  pendingInputs: PendingInput[];
  interruptedTurns: RecoveryTurn[];
  artifacts: Artifact[];
}
export interface WordTiming {
  text: string;
  startMs: number;
  durationMs: number;
}
export type OverlayEvent =
  | { type: "overlay_settings"; settings: OverlaySettings }
  | { type: "chatter_changed"; chatter: Chatter }
  | { type: "chat"; chatter: Chatter; text: string }
  | {
      type: "speech";
      id: string;
      chatter: Chatter | null;
      text: string;
      audio_path: string;
      words: WordTiming[];
    }
  | { type: "stop_speech" }
  | { type: "image"; id: string; image_path: string; title: string }
  | { type: "presentation"; title: string; text: string }
  | { type: "hide_image" }
  | { type: "status"; message: string };
