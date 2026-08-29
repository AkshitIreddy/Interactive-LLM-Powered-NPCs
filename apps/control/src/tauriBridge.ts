import type { ExecutionMode, StageId } from "./types";

export type NativeSimulationEvent =
  | {
      type: "started";
      simulationId: string;
      generation: number;
      sequence: number;
      measurementBasis:
        | "deterministicFixture"
        | "trustedRuntimeFixture"
        | "controlledBenchmark";
    }
  | {
      type: "stageStarted";
      simulationId: string;
      generation: number;
      sequence: number;
      stage: StageId;
      estimatedDurationMs: number;
    }
  | {
      type: "stageCompleted";
      simulationId: string;
      generation: number;
      sequence: number;
      stage: StageId;
      fixtureElapsedMs: number;
    }
  | {
      type: "sentenceReady";
      simulationId: string;
      generation: number;
      sequence: number;
      text: string;
    }
  | {
      type: "completed";
      simulationId: string;
      generation: number;
      sequence: number;
      fixtureFirstAudioMs: number;
      deliveredText: string;
    }
  | {
      type: "cancelled";
      simulationId: string;
      generation: number;
      sequence: number;
      reason: string;
    };

const hasTauri = () => "__TAURI_INTERNALS__" in window;

export async function startNativeSimulation(
  execution: ExecutionMode,
  onEvent: (event: NativeSimulationEvent) => void,
): Promise<boolean> {
  if (!hasTauri()) return false;
  const [{ invoke, Channel }] = await Promise.all([
    import("@tauri-apps/api/core"),
  ]);
  const events = new Channel<NativeSimulationEvent>();
  events.onmessage = onEvent;
  await invoke("start_simulation", {
    request: {
      gameProfileId: "eclipse-harbor",
      characterName: "Mara Venn",
      executionMode: execution,
    },
    events,
  });
  return true;
}

export async function cancelNativeSimulation(): Promise<boolean> {
  if (!hasTauri()) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("cancel_simulation");
  return true;
}
