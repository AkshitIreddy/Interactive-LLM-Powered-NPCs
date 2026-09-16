import { describe, expect, it } from "vitest";
import { ROUTE_OPTIONS } from "./providerLoadouts";

describe("testing dummy providers", () => {
  it("lists a local echo LLM that spends no API calls", () => {
    const testing = ROUTE_OPTIONS.llm.find((provider) => provider.id === "testing");
    expect(testing).toBeDefined();
    expect(testing?.execution).toBe("Local");
    expect(testing?.models.map((model) => model.id)).toEqual(["testing-echo-v1"]);
  });

  it("lists a local synth voice that spends no API calls", () => {
    const testing = ROUTE_OPTIONS.tts.find((provider) => provider.id === "testing");
    expect(testing).toBeDefined();
    expect(testing?.execution).toBe("Local");
    expect(testing?.models.map((model) => model.id)).toEqual(["testing-synth-v1"]);
    expect(testing?.privacy).toMatch(/stays on this pc/i);
  });
});
