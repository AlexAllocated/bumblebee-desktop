import { test, expect } from "bun:test";
import {
  createSettingsWriter,
  mergeSettings,
  settingsDiff,
} from "./settingsWriter";

test("sparse edits preserve unrelated native changes and later slider gestures", async () => {
  let value = { volume: 1, policy: { everyone: false }, model: "first" };
  let resolve!: (v: typeof value) => void;
  const patches: any[] = [];
  const writer = createSettingsWriter({
    read: () => value,
    apply: (next) => {
      value = next;
    },
    status() {},
    persist: async (patch) => {
      patches.push(patch);
      if (patches.length === 1)
        return new Promise<typeof value>((r) => {
          resolve = r;
        });
      return mergeSettings({ ...value, model: "native-change" }, patch);
    },
  });
  writer.accept(value);
  value.volume = 0.5;
  const saving = writer.flush();
  value.volume = 0.75;
  resolve({ volume: 0.5, policy: { everyone: false }, model: "native-change" });
  await saving;
  expect(patches).toEqual([{ volume: 0.5 }, { volume: 0.75 }]);
  expect(value).toEqual({
    volume: 0.75,
    policy: { everyone: false },
    model: "native-change",
  });
  writer.dispose();
});

test("failed persistence retains the draft and can be retried", async () => {
  let value = { enabled: false };
  let fail = true;
  const writer = createSettingsWriter({
    read: () => value,
    apply: (v) => {
      value = v;
    },
    status() {},
    persist: async (p) => {
      if (fail) throw new Error("disk unavailable");
      return mergeSettings({ enabled: false }, p);
    },
  });
  writer.accept(value);
  value.enabled = true;
  await expect(writer.flush()).rejects.toThrow("disk unavailable");
  expect(value.enabled).toBe(true);
  fail = false;
  await writer.flush();
  expect(value.enabled).toBe(true);
  writer.dispose();
});

test("arrays are replaced as values and nested policy edits stay sparse", () => {
  expect(
    settingsDiff(
      { roles: ["a"], policy: { everyone: false, mods: false } },
      { roles: [], policy: { everyone: false, mods: true } },
    ),
  ).toEqual({ roles: [], policy: { mods: true } });
});

test("disposed writer never applies a late save or refresh to a removed UI", async () => {
  let value = { volume: 1 };
  let resolve!: (next: typeof value) => void;
  let applied = 0;
  const statuses: string[] = [];
  const writer = createSettingsWriter({
    read: () => value,
    apply: (next) => {
      value = next;
      applied++;
    },
    status: (status) => statuses.push(status),
    persist: () =>
      new Promise<typeof value>((r) => {
        resolve = r;
      }),
  });
  writer.accept(value);
  value.volume = 0.5;
  const saving = writer.flush();
  writer.dispose();
  writer.accept({ volume: 0.8 });
  resolve({ volume: 0.5 });
  await saving;
  expect(applied).toBe(1);
  expect(statuses).toEqual(["saving"]);
});

test("an emptied numeric field remains a visible invalid draft through refresh and can be corrected", async () => {
  let value: { port: number | undefined; model: string } = {
    port: 2899,
    model: "first",
  };
  const errors: string[] = [];
  let saves = 0;
  const writer = createSettingsWriter({
    read: () => value,
    apply: (next) => {
      value = next;
    },
    status: (state, error) => {
      if (error) errors.push(error);
    },
    persist: async (patch) => {
      saves++;
      return mergeSettings({ port: 2899, model: "native" }, patch);
    },
  });
  writer.accept(value);
  value.port = undefined;
  await expect(writer.flush()).rejects.toThrow("port needs a valid value");
  expect(saves).toBe(0);
  expect(errors.length).toBe(1);
  writer.accept({ port: 2899, model: "native" });
  expect(value.port).toBeUndefined();
  expect(value.model).toBe("native");
  value.port = 3000;
  await writer.flush();
  expect(value.port).toBe(3000);
  expect(saves).toBe(1);
  writer.dispose();
});

test("refresh during a pending save preserves an explicit re-block back to baseline", async () => {
  let value = { policy: { everyone: false }, volume: 1 };
  let resolve!: (saved: typeof value) => void;
  const writes: Record<string, unknown>[] = [];
  const writer = createSettingsWriter({
    read: () => value,
    apply: (next) => {
      value = next;
    },
    status: () => {},
    persist: async (patch) => {
      writes.push(patch);
      if (writes.length === 1)
        return new Promise<typeof value>((r) => {
          resolve = r;
        });
      return mergeSettings({ policy: { everyone: true }, volume: 1 }, patch);
    },
  });
  writer.accept(value);
  value.policy.everyone = true;
  const saving = writer.flush();
  value.policy.everyone = false;
  writer.accept({ policy: { everyone: true }, volume: 0.8 });
  expect(value.policy.everyone).toBe(false);
  // A second refresh updates unrelated native changes instead of mistaking them for local edits.
  writer.accept({ policy: { everyone: true }, volume: 0.7 });
  expect(value.volume).toBe(0.7);
  resolve({ policy: { everyone: true }, volume: 0.7 });
  await saving;
  expect(value.policy.everyone).toBe(false);
  expect(writes).toEqual([
    { policy: { everyone: true } },
    { policy: { everyone: false } },
  ]);
  writer.dispose();
});

test("a stale refresh cannot erase the edit currently being saved", async () => {
  let value = { volume: 1 };
  let resolve!: (saved: typeof value) => void;
  const writes: Record<string, unknown>[] = [];
  const writer = createSettingsWriter({
    read: () => value,
    apply: (next) => {
      value = next;
    },
    status: () => {},
    persist: async (patch) => {
      writes.push(patch);
      return new Promise<typeof value>((r) => {
        resolve = r;
      });
    },
  });
  writer.accept(value);
  value.volume = 0.5;
  const saving = writer.flush();
  writer.accept({ volume: 1 });
  expect(value.volume).toBe(0.5);
  resolve({ volume: 0.5 });
  await saving;
  expect(value.volume).toBe(0.5);
  expect(writes).toEqual([{ volume: 0.5 }]);
  writer.dispose();
});
