import type { PreparedSpeechAudio, Speech } from "./overlay/types";
export type PreparedSpeechAssetResponse = PreparedSpeechAudio;
export type PrepareSpeechAssetOptions = {
  speech: Speech;
  prepare: (
    speech: Speech,
    signal: AbortSignal,
  ) => Promise<PreparedSpeechAssetResponse>;
  signal?: AbortSignal;
};
/** The host prepares speech; this rendering package never stores credentials or calls a provider. */
export async function prepareSpeechAsset(
  options: PrepareSpeechAssetOptions,
): Promise<PreparedSpeechAssetResponse> {
  const signal = options.signal ?? new AbortController().signal;
  signal.throwIfAborted();
  const prepared = await options.prepare(options.speech, signal);
  signal.throwIfAborted();
  return prepared;
}
