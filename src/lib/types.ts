import type { OverlaySettings } from "./overlay";
export type { OverlaySettings } from "./overlay";
export interface AudiencePolicy {
  everyone: boolean;
  roleIds: string[];
  followers: boolean;
  subscribers: boolean;
  vips: boolean;
  moderators: boolean;
  members: boolean;
}
export interface PlatformPolicy {
  monitor: boolean;
  relay: boolean;
  mentions: AudiencePolicy;
  readout: AudiencePolicy;
}
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
  audioOutput: "overlay" | "discord";
  masterVolume: number;
  bumblebeeTtsVolume: number;
  puppetTtsVolume: number;
  wakeChirpVolume: number;
  thinkingSoundVolume: number;
  flyingSoundVolume: number;
  chatTtsWaitingToneVolume: number;
  chatTtsWaitingToneEnabled: boolean;
  chatTtsSpeakerIntroCooldownSeconds: number;
  chatTtsInterruptSilenceMs: number;
  chatTtsQueueExpirationMs: number;
  chatTtsBlockedWords: string[];
  chatAiDictationEnabled: boolean;
  voiceMentionsEnabled: boolean;
  wakeKeywordSensitivity: "strict" | "balanced" | "loose";
  stopKeywordSensitivity: "strict" | "balanced" | "loose";
  cancelKeywordSensitivity: "strict" | "balanced" | "loose";
  openaiVoiceModel: string;
  openaiReasoningEffort: string;
  openaiVoiceReasoningEffort: string;
  imageModel: string;
  aiWebSearchEnabled: boolean;
  aiCodeInterpreterEnabled: boolean;
  aiImageGenerationEnabled: boolean;
  aiMemoriesEnabled: boolean;
  aiRemindersEnabled: boolean;
  chatPlatforms: Record<"discord" | "twitch" | "youtube", PlatformPolicy>;
  voiceCaptionsEnabled: boolean;
  streamerTranscriptionModel: string;
  voiceTranscriptionModel: string;
}
export type OverrideChoice = "inherit" | "allow" | "block";
export interface ChatterOverrides {
  chatPuppet: OverrideChoice;
  relay: OverrideChoice;
  ttsWait: OverrideChoice;
  aiAccess: OverrideChoice;
  textModel: string | null;
  voiceModel: string | null;
}
export interface Chatter {
  platform: string;
  userId: string;
  displayName: string;
  puppetId: string;
  voiceId: string;
  imageHash: string | null;
  customizationBlocked: boolean;
  overrides: ChatterOverrides;
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
export interface RecoveryReceipt {
  status: string;
  destination: string | null;
  messageIds: string[];
  completedParts: number | null;
  totalParts: number | null;
  unacknowledgedPartMayHaveSent: boolean | null;
}
export interface RecoveryTurn {
  id: string;
  actor: string;
  state: string;
  updatedAt: number;
  resumable: boolean;
  receipts: RecoveryReceipt[];
}
export interface Memory {
  id: string;
  content: string;
  actor: string;
  createdAt: number;
}
export interface Reminder {
  id: string;
  content: string;
  actor: string;
  dueAt: number;
  state: string;
}
export interface Library {
  memories: Memory[];
  reminders: Reminder[];
}
export interface DiscordOption {
  id: string;
  name: string;
  kind: number;
  parentId: string | null;
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
export interface AudioMix {
  output: "overlay" | "discord";
  masterVolume: number;
  bumblebeeTtsVolume: number;
  puppetTtsVolume: number;
  flyingSoundVolume: number;
  wakeChirpVolume: number;
  thinkingSoundVolume: number;
  chatTtsWaitingToneVolume: number;
}
export type OverlayEvent =
  | {
      type: "signal";
      id: string;
      audio_path: string;
      gain: number;
      kind: "wake" | "heard" | "timeout" | "cancel" | "thinking" | "waiting";
      looping: boolean;
      audible: boolean;
    }
  | { type: "stop_signal"; id: string }
  | { type: "audio_settings"; settings: AudioMix }
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
      audible: boolean;
      gain: number;
    }
  | { type: "stop_speech" }
  | {
      type: "voice_transcript";
      userId: string;
      isOwner: boolean;
      text: string;
      final: boolean;
    }
  | { type: "image"; id: string; image_path: string; title: string }
  | { type: "presentation"; title: string; text: string }
  | { type: "hide_image" }
  | { type: "status"; message: string };
