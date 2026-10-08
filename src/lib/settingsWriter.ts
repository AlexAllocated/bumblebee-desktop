export type JsonObject = Record<string, any>;
// Svelte proxies cannot be structuredClone'd. Keep unfinished form values in
// the local draft so validation can report them instead of silently discarding.
function copy<T>(value: T): T {
  if (Array.isArray(value)) return value.map(copy) as T;
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, copy(item)]),
    ) as T;
  return value;
}
function validateDraft(value: unknown, path = "Settings"): void {
  if (
    value === undefined ||
    (typeof value === "number" && !Number.isFinite(value))
  )
    throw new Error(`${path} needs a valid value.`);
  if (value && typeof value === "object")
    for (const [key, item] of Object.entries(value))
      validateDraft(item, `${path} → ${key}`);
}

/** Sparse edits preserve settings changed concurrently by the native agent. */
export function settingsDiff(
  before: JsonObject,
  after: JsonObject,
): JsonObject {
  const result: JsonObject = {};
  for (const [key, value] of Object.entries(after)) {
    if (JSON.stringify(before[key]) === JSON.stringify(value)) continue;
    if (
      value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      before[key] &&
      typeof before[key] === "object"
    ) {
      const nested = settingsDiff(before[key], value);
      if (Object.keys(nested).length) result[key] = nested;
    } else result[key] = copy(value);
  }
  return result;
}
export function mergeSettings<T>(base: T, patch: JsonObject): T {
  return Object.fromEntries(
    Object.entries(base as JsonObject).map(([key, value]) => [
      key,
      !(key in patch)
        ? value
        : value &&
            typeof value === "object" &&
            !Array.isArray(value) &&
            patch[key] &&
            typeof patch[key] === "object"
          ? mergeSettings(value, patch[key])
          : copy(patch[key]),
    ]),
  ) as T;
}

// Patches may introduce different edited leaves; unlike mergeSettings this
// combines their key sets, with the later edit winning even a reverted value.
function combinePatches(before: JsonObject, after: JsonObject): JsonObject {
  const result = copy(before);
  for (const [key, value] of Object.entries(after)) {
    result[key] =
      value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      result[key] &&
      typeof result[key] === "object" &&
      !Array.isArray(result[key])
        ? combinePatches(result[key], value)
        : copy(value);
  }
  return result;
}

/** A single writer keeps later gestures when an earlier save completes. */
export function createSettingsWriter<T extends JsonObject>(options: {
  read: () => T;
  apply: (settings: T) => void;
  persist: (patch: JsonObject) => Promise<T>;
  status: (
    state: "idle" | "saving" | "saved" | "error",
    error?: string,
  ) => void;
}) {
  let baseline: T | undefined;
  let pending: Promise<void> | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  let inFlight:
    { patch: JsonObject; later: JsonObject; observed: T } | undefined;
  function captureLaterEdits() {
    if (inFlight) {
      inFlight.later = combinePatches(
        inFlight.later,
        settingsDiff(inFlight.observed, options.read()),
      );
      inFlight.observed = clone(options.read());
    }
  }
  const clone = (value: T) => copy(value);
  function accept(value: T) {
    if (disposed) return;
    captureLaterEdits();
    const edits = inFlight
      ? combinePatches(inFlight.patch, inFlight.later)
      : baseline
        ? settingsDiff(baseline, options.read())
        : {};
    baseline = clone(value);
    const draft = mergeSettings(value, edits);
    options.apply(draft);
    if (inFlight) inFlight.observed = clone(draft);
  }
  async function flush() {
    clearTimeout(timer);
    if (pending) {
      await pending;
      return flush();
    }
    if (!baseline || disposed) return;
    const task = async () => {
      while (!disposed) {
        const submitted = clone(options.read());
        const patch = settingsDiff(baseline!, submitted);
        if (!Object.keys(patch).length) return;
        options.status("saving");
        try {
          validateDraft(submitted);
          inFlight = {
            patch: copy(patch),
            later: {},
            observed: clone(submitted),
          };
          const saved = await options.persist(patch);
          if (disposed) return;
          captureLaterEdits();
          const newerEdits = inFlight.later;
          inFlight = undefined;
          baseline = clone(saved);
          options.apply(mergeSettings(saved, newerEdits));
          options.status("saved");
        } catch (error) {
          inFlight = undefined;
          if (!disposed) options.status("error", String(error));
          throw error;
        }
      }
    };
    pending = task();
    try {
      await pending;
    } finally {
      pending = undefined;
    }
  }
  return {
    accept,
    flush,
    schedule() {
      clearTimeout(timer);
      timer = setTimeout(() => {
        void flush().catch(() => {});
      }, 350);
    },
    dispose() {
      disposed = true;
      clearTimeout(timer);
    },
  };
}
