export type CredentialStatus =
  | "present"
  | "missing"
  | "notRequired"
  | "unavailable";

export interface ProviderCredentialSummary {
  providerId: string;
  displayName: string;
  credentialReference: string | null;
  status: CredentialStatus;
  detail: string;
}

export interface CredentialActionResult {
  providerId: string;
  status: CredentialStatus;
  detail: string;
  networkRequestPerformed?: false;
  outcome?: "saved" | "cancelled" | "developmentFixtureOnly";
}
interface PromptCredentialResult {
  providerId: string;
  outcome: "saved" | "cancelled" | "developmentFixtureOnly";
  credentialStatus: CredentialStatus;
  detail: string;
}

interface ProviderConnectionTestResult {
  providerId: string;
  outcome:
    | "readyForConnection"
    | "needsCredential"
    | "providerContractUnavailable"
    | "runtimeUnavailable";
  credentialStatus: CredentialStatus;
  runtimeConnected: boolean;
  providerContractAvailable: boolean;
  networkRequestPerformed: false;
  responseBodyReturned: false;
  detail: string;
}

const browserConfiguredProviders = new Set(["openai"]);
let lastBrowserCommand: {
  command: string;
  args: Record<string, string>;
} | null = null;
const isTauri = () => "__TAURI_INTERNALS__" in window;

const browserStatuses = (): ProviderCredentialSummary[] =>
  HOSTED_PROVIDER_IDS.map((providerId) => ({
    providerId,
    displayName: providerId,
    credentialReference: browserConfiguredProviders.has(providerId)
      ? `fixture/providers/${providerId}`
      : null,
    status: browserConfiguredProviders.has(providerId) ? "present" : "missing",
    detail: browserConfiguredProviders.has(providerId)
      ? "A fake browser-fixture reference is available."
      : "No fake browser-fixture reference is configured.",
  }));

export async function listProviderCredentialStatus(): Promise<
  ProviderCredentialSummary[]
> {
  if (!isTauri()) return browserStatuses();
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProviderCredentialSummary[]>("provider_credential_status", {
    providerId: null,
  });
}

export async function promptAndSaveProviderCredential(
  providerId: string,
): Promise<CredentialActionResult> {
  if (!isTauri()) {
    await Promise.resolve();
    lastBrowserCommand = {
      command: "prompt_and_save_provider_credential",
      args: { providerId },
    };
    browserConfiguredProviders.add(providerId);
    return {
      providerId,
      status: "present",
      detail:
        "Browser fixture modeled completion of a native prompt. No credential entered the page.",
      outcome: "developmentFixtureOnly",
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke<PromptCredentialResult>(
    "prompt_and_save_provider_credential",
    { providerId },
  );
  return {
    providerId: result.providerId,
    status: result.credentialStatus,
    detail: result.detail,
    outcome: result.outcome,
  };
}

export async function testProviderCredential(
  providerId: string,
): Promise<CredentialActionResult> {
  if (!isTauri()) {
    await Promise.resolve();
    if (!browserConfiguredProviders.has(providerId))
      throw new Error("Save a credential before testing its reference.");
    lastBrowserCommand = {
      command: "test_provider_connection",
      args: { providerId },
    };
    return {
      providerId,
      status: "present",
      detail:
        "Saved reference and provider contract are present. No provider request was made.",
      networkRequestPerformed: false,
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke<ProviderConnectionTestResult>(
    "test_provider_connection",
    { providerId },
  );
  if (result.outcome !== "readyForConnection") throw new Error(result.detail);
  return {
    providerId: result.providerId,
    status: result.credentialStatus,
    detail: `${result.detail} No provider request was made.`,
    networkRequestPerformed: false,
  };
}

export async function deleteProviderCredential(
  providerId: string,
): Promise<CredentialActionResult> {
  if (!isTauri()) {
    await Promise.resolve();
    browserConfiguredProviders.delete(providerId);
    return {
      providerId,
      status: "missing",
      detail: "Fake browser-fixture reference removed.",
    };
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<CredentialActionResult>("delete_provider_credential", {
    providerId,
  });
}

export const HOSTED_PROVIDER_IDS = [
  "openai",
  "gemini",
  "anthropic",
  "cohere",
  "nvidia-nim",
  "groq",
  "mistral",
  "openrouter",
  "deepgram",
  "assemblyai",
  "elevenlabs",
  "cartesia",
  "inworld",
] as const;

export function resetBrowserProviderFixtureForTests() {
  browserConfiguredProviders.clear();
  browserConfiguredProviders.add("openai");
  lastBrowserCommand = null;
}

export function getLastBrowserProviderCommandForTests() {
  return lastBrowserCommand;
}
