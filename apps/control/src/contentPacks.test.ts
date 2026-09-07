import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  activateContentPack,
  inspectContentPackJson,
  readContentPackState,
} from "./contentPacks";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

describe("content-pack native bridge", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: {},
    });
  });

  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("sends the exact reviewed bytes and digest without a file path", async () => {
    invokeMock.mockResolvedValue({});
    await inspectContentPackJson("exact-json");
    await activateContentPack("exact-json", "a".repeat(64));
    expect(invokeMock).toHaveBeenNthCalledWith(1, "inspect_content_pack", {
      request: { jsonText: "exact-json" },
    });
    expect(invokeMock).toHaveBeenNthCalledWith(2, "activate_content_pack", {
      request: {
        jsonText: "exact-json",
        expectedContentSha256: "a".repeat(64),
      },
    });
  });

  it("keeps browser preview read-only", async () => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    await expect(readContentPackState()).resolves.toBeNull();
    await expect(inspectContentPackJson("{}")).rejects.toThrow(
      /installed Windows app/,
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
