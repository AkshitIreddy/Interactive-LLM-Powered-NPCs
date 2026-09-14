import { useEffect, useMemo, useState } from "react";
import {
  Button,
  ModalOverlay,
  Modal,
  Dialog,
  Heading,
} from "react-aria-components";
import { CyberwareAnatomy } from "./CyberwareAnatomy";
import { ActionButton, StatusPill } from "./components";
import { Icon } from "./icons";
import "./loadout.css";
import {
  makeLoadout,
  modelFor,
  providerFor,
  readBrowserLoadouts,
  ROLE_META,
  ROLE_ORDER,
  ROUTE_OPTIONS,
  writeBrowserLoadouts,
  type LoadoutScope,
  type ProviderLoadout,
  type ProviderRole,
  type RouteChoice,
} from "./providerLoadouts";
import {
  fromNativeSnapshot,
  nativeActivate,
  nativeAcknowledgePrivateEvaluation,
  nativeClone,
  nativeCreate,
  nativeDeactivateScope,
  nativeDelete,
  nativeDiscoverTtsStockVoices,
  nativePrivateEvaluationAcknowledgement,
  nativePrivateEvaluationPolicy,
  nativeRename,
  nativeReview,
  nativeSnapshot,
  nativeUpdate,
  type NativeLoadoutDocument,
  type NativeLoadoutSnapshot,
  type NativeProviderLoadoutReview,
  type NativeProviderPrivateEvaluationAcknowledgement,
  type NativeProviderPrivateEvaluationPolicy,
  type NativeTtsStockVoiceDiscovery,
} from "./providerLoadoutBridge";

const FALLBACK_ROLES: ProviderRole[] = [
  "llm",
  "stt",
  "tts",
  "embeddings",
  "vision",
];

export interface ProviderLoadoutEditorProps {
  gameProfileId?: string;
  gameProfileLabel?: string;
  characterId?: string;
  characterLabel?: string;
  initialRole?: ProviderRole;
  mode?: "full" | "onboarding";
  onManageProvider?: (providerId: string) => void;
  onManageMouthMotion?: () => void;
  onNativeLoadoutsChange?: (loadouts: ProviderLoadout[]) => void;
}

function routeIsExecutable(role: ProviderRole, providerId: string) {
  if (role === "stt") return providerId === "assemblyai";
  if (role === "vision" || role === "lipSync") return providerId === "disabled";
  return providerFor(role, providerId).selectable !== false;
}

function accountProviderId(providerId: string) {
  return providerId.startsWith("nvidia-nim") ? "nvidia-nim" : providerId;
}

function qualifiedStockVoiceId(providerId: string) {
  switch (providerId) {
    case "elevenlabs":
      return "EXAVITQu4vr4xnSDxMaL";
    case "cartesia":
      return "a0e99841-438c-4a64-b679-ae501e7d6091";
    case "deepgram":
      return "Arcas";
    case "inworld":
      return "Dennis";
    default:
      return undefined;
  }
}

function stockVoicePresetFor(providerId: string) {
  const voiceId = qualifiedStockVoiceId(providerId);
  if (!voiceId) return null;
  const labels: Record<string, string> = {
    elevenlabs: "Sarah · warm conversation",
    cartesia: "Greg · clear and grounded",
    deepgram: "Arcas · natural English",
    inworld: "Dennis · expressive English",
  };
  return { voiceId, label: labels[providerId] ?? "Verified stock voice" };
}

const scopeCopy: Record<
  LoadoutScope,
  { label: string; eyebrow: string; description: string }
> = {
  global: {
    label: "Global",
    eyebrow: "01 · BASE ROUTE",
    description: "The default for every conversation.",
  },
  game: {
    label: "Game",
    eyebrow: "02 · GAME OVERRIDE",
    description: "Replaces the global route for one game.",
  },
  character: {
    label: "Character",
    eyebrow: "03 · CHARACTER OVERRIDE",
    description: "The narrowest route wins for one character.",
  },
};

function scopeKey(loadout: ProviderLoadout) {
  return `${loadout.scope}:${loadout.targetId ?? "*"}`;
}

function defaultFallback(role: ProviderRole, primary: RouteChoice) {
  const provider = ROUTE_OPTIONS[role].find(
    (candidate) =>
      candidate.id !== primary.providerId &&
      candidate.execution !== "Off" &&
      candidate.selectable !== false &&
      routeIsExecutable(role, candidate.id),
  );
  const selected = provider ?? ROUTE_OPTIONS[role][0];
  return {
    providerId: selected.id,
    modelId: selected.models[0].id,
    authorized: false,
  };
}

export function ProviderLoadoutEditor({
  gameProfileId,
  gameProfileLabel,
  characterId,
  characterLabel,
  initialRole = "llm",
  mode = "full",
  onManageProvider,
  onManageMouthMotion,
  onNativeLoadoutsChange,
}: ProviderLoadoutEditorProps = {}) {
  const [loadouts, setLoadouts] =
    useState<ProviderLoadout[]>(readBrowserLoadouts);
  const [selectedId, setSelectedId] = useState(loadouts[0].id);
  const [scopeFilter, setScopeFilter] = useState<LoadoutScope>(
    loadouts[0].scope,
  );
  const [notice, setNotice] = useState(
    "Choose a system to configure its provider and model.",
  );
  const [nativeDocument, setNativeDocument] =
    useState<NativeLoadoutDocument | null>(null);
  const [nativeCatalogRevision, setNativeCatalogRevision] = useState<
    number | null
  >(null);
  const [reviewOffline, setReviewOffline] = useState(false);
  const [review, setReview] = useState<NativeProviderLoadoutReview | null>(
    null,
  );
  const [reviewBusy, setReviewBusy] = useState(false);
  const [stockVoices, setStockVoices] =
    useState<NativeTtsStockVoiceDiscovery | null>(null);
  const [stockVoicesBusy, setStockVoicesBusy] = useState(false);
  const [stockVoicesError, setStockVoicesError] = useState<string | null>(null);
  const [
    privateEvaluationAcknowledgement,
    setPrivateEvaluationAcknowledgement,
  ] = useState<NativeProviderPrivateEvaluationAcknowledgement | null>(null);
  const [privateEvaluationPolicy, setPrivateEvaluationPolicy] =
    useState<NativeProviderPrivateEvaluationPolicy | null>(null);
  const [privateEvaluationPolicyError, setPrivateEvaluationPolicyError] =
    useState<string | null>(null);
  const [privateEvaluationConsentChecked, setPrivateEvaluationConsentChecked] =
    useState(false);
  const [privateEvaluationArmed, setPrivateEvaluationArmed] = useState(false);
  const [privateEvaluationBusy, setPrivateEvaluationBusy] = useState(false);
  const [activeRole, setActiveRole] = useState<ProviderRole>(initialRole);
  const [customVoiceEditing, setCustomVoiceEditing] = useState(false);
  const [managerOpen, setManagerOpen] = useState(false);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const selected =
    loadouts.find((loadout) => loadout.id === selectedId) ?? loadouts[0];
  const selectedNvidiaModalities = [
    ...(selected.routes.llm.providerId === "nvidia-nim" ? ["LLM"] : []),
    ...(selected.routes.embeddings.providerId === "nvidia-nim"
      ? ["embeddings"]
      : []),
    ...(selected.routes.tts.providerId === "nvidia-nim-magpie"
      ? ["Magpie TTS"]
      : []),
  ];
  const privateEvaluationReady = Boolean(
    privateEvaluationPolicy?.namespaceEligible &&
      privateEvaluationPolicy.catalogRevision === nativeCatalogRevision &&
      privateEvaluationAcknowledgement &&
      privateEvaluationAcknowledgement.providerId ===
        privateEvaluationPolicy.providerId &&
      privateEvaluationAcknowledgement.termsRevision ===
        privateEvaluationPolicy.termsRevision &&
      privateEvaluationAcknowledgement.catalogRevision ===
        privateEvaluationPolicy.catalogRevision &&
      privateEvaluationAcknowledgement.applicationNamespace ===
        privateEvaluationPolicy.applicationNamespace,
  );
  const contextTargetId =
    scopeFilter === "game"
      ? gameProfileId
      : scopeFilter === "character" && gameProfileId && characterId
        ? `${gameProfileId}/${characterId}`
        : undefined;
  const currentScopeLoadouts = loadouts.filter(
    (loadout) =>
      loadout.scope === scopeFilter &&
      (scopeFilter === "global" ||
        (Boolean(contextTargetId) && loadout.targetId === contextTargetId)),
  );
  const cloudVendors = useMemo(
    () =>
      new Set(
        ROLE_ORDER.map((role) =>
          providerFor(role, selected.routes[role].providerId),
        )
          .filter((provider) => provider.execution === "Cloud")
          .map((provider) => provider.name),
      ),
    [selected],
  );
  const localRoles = ROLE_ORDER.filter(
    (role) =>
      providerFor(role, selected.routes[role].providerId).execution === "Local",
  );
  const activeForScope = loadouts.find(
    (loadout) => loadout.active && scopeKey(loadout) === scopeKey(selected),
  );
  const fallbackRoles = FALLBACK_ROLES.filter((role) =>
    ROUTE_OPTIONS[role].some(
      (provider) =>
        provider.id !== selected.routes[role].providerId &&
        provider.execution !== "Off" &&
        provider.selectable !== false &&
        routeIsExecutable(role, provider.id),
    ),
  );
  const targetForScope = (scope: LoadoutScope) => {
    if (scope === "global") {
      return { targetId: undefined, targetLabel: "Every game" };
    }
    if (!gameProfileId) return null;
    const resolvedGameLabel = gameProfileLabel ?? gameProfileId;
    if (scope === "game") {
      return { targetId: gameProfileId, targetLabel: resolvedGameLabel };
    }
    if (!characterId) return null;
    return {
      targetId: `${gameProfileId}/${characterId}`,
      targetLabel: `${resolvedGameLabel} · ${characterLabel ?? characterId}`,
    };
  };
  const targetLabelFor = (loadout: ProviderLoadout) => {
    if (loadout.scope === "global") return "Every game";
    if (loadout.scope === "game" && loadout.targetId === gameProfileId) {
      return gameProfileLabel ?? gameProfileId ?? loadout.targetLabel;
    }
    if (
      loadout.scope === "character" &&
      loadout.targetId === `${gameProfileId}/${characterId}`
    ) {
      return `${gameProfileLabel ?? gameProfileId} · ${characterLabel ?? characterId}`;
    }
    return loadout.targetLabel ?? loadout.targetId ?? "Unknown scope";
  };
  const chooseScope = (scope: LoadoutScope) => {
    setScopeFilter(scope);
    const target = targetForScope(scope);
    const scoped = loadouts.find(
      (loadout) =>
        loadout.scope === scope &&
        (scope === "global" || loadout.targetId === target?.targetId),
    );
    if (scoped) chooseLoadout(scoped);
  };

  const acceptNativeSnapshot = (
    snapshot: NativeLoadoutSnapshot,
    message?: string,
  ) => {
    const nativeLoadouts = fromNativeSnapshot(snapshot);
    if (nativeLoadouts.length === 0) return;
    const preferred =
      nativeLoadouts.find((loadout) => loadout.id === selectedId) ??
      nativeLoadouts[0];
    setNativeDocument(snapshot.document);
    setNativeCatalogRevision(snapshot.catalogRevision);
    setLoadouts(nativeLoadouts);
    onNativeLoadoutsChange?.(nativeLoadouts);
    setSelectedId(preferred.id);
    setScopeFilter(preferred.scope);
    setNotice(
      message ??
        `${snapshot.detail} Credentials were not checked and no provider request was made.`,
    );
  };

  const nativeFailure = (action: string) => {
    setNotice(
      `${action} was not committed by the native runtime. Browser preview state remains visible; no provider request was made.`,
    );
  };

  useEffect(() => {
    let current = true;
    nativeSnapshot()
      .then(async (snapshot) => {
        if (!current || !snapshot) return;
        acceptNativeSnapshot(snapshot);
        setStockVoicesBusy(true);
        let policy: NativeProviderPrivateEvaluationPolicy | null = null;
        try {
          policy = await nativePrivateEvaluationPolicy();
          if (current) {
            setPrivateEvaluationPolicy(policy);
            setPrivateEvaluationPolicyError(null);
          }
        } catch (error: unknown) {
          if (current) {
            setPrivateEvaluationPolicy(null);
            setPrivateEvaluationPolicyError(
              error instanceof Error
                ? error.message
                : "Native private-evaluation policy is unavailable.",
            );
          }
        }
        try {
          const acknowledgement =
            await nativePrivateEvaluationAcknowledgement();
          if (current) {
            setPrivateEvaluationAcknowledgement(
              acknowledgement &&
                policy &&
                acknowledgement.providerId === policy.providerId &&
                acknowledgement.termsRevision === policy.termsRevision &&
                acknowledgement.catalogRevision === policy.catalogRevision &&
                acknowledgement.applicationNamespace ===
                  policy.applicationNamespace
                ? acknowledgement
                : null,
            );
          }
        } catch {
          if (current) setPrivateEvaluationAcknowledgement(null);
        }
        if (!current) return;
        try {
          const stockVoiceResult = await nativeDiscoverTtsStockVoices(false);
          if (!current) return;
          setStockVoices(stockVoiceResult);
          setStockVoicesError(stockVoiceResult.error?.detail ?? null);
        } catch (error: unknown) {
          if (!current) return;
          setStockVoicesError(
            error instanceof Error
              ? error.message
              : "NVIDIA stock voices could not be discovered.",
          );
        }
        setStockVoicesBusy(false);
      })
      .catch(() => {
        if (current) nativeFailure("Native loadout snapshot");
      });
    return () => {
      current = false;
    };
    // The native snapshot is read once; later changes return their own snapshot.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const commit = (
    next: ProviderLoadout[],
    message: string,
    nextSelectedId = selectedId,
  ) => {
    setLoadouts(next);
    setSelectedId(nextSelectedId);
    writeBrowserLoadouts(next);
    setNotice(message);
  };

  const updateSelected = (
    update: (loadout: ProviderLoadout) => ProviderLoadout,
    message: string,
    syncNative = true,
  ) => {
    const next = loadouts.map((loadout) =>
      loadout.id === selected.id
        ? {
            ...update(structuredClone(loadout)),
            revision: loadout.revision + 1,
          }
        : loadout,
    );
    const changed = next.find((loadout) => loadout.id === selected.id);
    if (
      syncNative &&
      changed &&
      nativeDocument &&
      nativeCatalogRevision !== null
    ) {
      setNotice("Saving route change to protected native state…");
      nativeUpdate(changed, nativeDocument, nativeCatalogRevision)
        .then((snapshot) => acceptNativeSnapshot(snapshot, message))
        .catch(() => nativeFailure("Loadout update"));
      return;
    }
    commit(next, message);
  };

  const chooseLoadout = (loadout: ProviderLoadout) => {
    setSelectedId(loadout.id);
    setScopeFilter(loadout.scope);
    setReview(null);
    setNotice(
      loadout.active
        ? "This is the active preference for its scope. A running turn keeps its original route snapshot."
        : "Reviewing an inactive loadout. Nothing is routed until you activate it.",
    );
  };

  const addLoadout = () => {
    const target = targetForScope(scopeFilter);
    if (!target) {
      setNotice(
        scopeFilter === "game"
          ? "Select a game in World before creating a game override."
          : "Select a game and character in World before creating a character override.",
      );
      return;
    }
    const created = {
      ...makeLoadout(scopeFilter, loadouts.length + 1),
      ...target,
    };
    commit(
      [...loadouts, created],
      "New loadout created locally. Name it, review every role, then activate it explicitly.",
      created.id,
    );
    if (nativeDocument && nativeCatalogRevision !== null) {
      nativeCreate(created, nativeDocument, nativeCatalogRevision)
        .then((snapshot) =>
          acceptNativeSnapshot(
            snapshot,
            "Loadout created in protected native state.",
          ),
        )
        .catch(() => nativeFailure("Loadout creation"));
    }
  };

  const cloneLoadout = () => {
    const clone: ProviderLoadout = {
      ...structuredClone(selected),
      id: `loadout-${Date.now()}-clone`,
      name: `${selected.name} copy`,
      active: false,
      revision: 1,
    };
    commit(
      [...loadouts, clone],
      "Cloned as an inactive loadout. Current and in-flight routes are unchanged.",
      clone.id,
    );
    if (nativeDocument) {
      nativeClone(selected.id, clone.id, clone.name)
        .then((snapshot) =>
          acceptNativeSnapshot(
            snapshot,
            "Loadout cloned in protected native state.",
          ),
        )
        .catch(() => nativeFailure("Loadout clone"));
    }
  };

  const deleteLoadout = () => {
    if (loadouts.length === 1) return;
    const next = loadouts.filter((loadout) => loadout.id !== selected.id);
    const replacement =
      next.find((loadout) => loadout.scope === scopeFilter) ?? next[0];
    commit(
      next,
      "Loadout deleted. No provider credentials were changed.",
      replacement.id,
    );
    setScopeFilter(replacement.scope);
    if (nativeDocument) {
      nativeDelete(selected.id)
        .then((snapshot) =>
          acceptNativeSnapshot(
            snapshot,
            "Loadout deleted from protected native state.",
          ),
        )
        .catch(() => nativeFailure("Loadout deletion"));
    }
  };

  const activateLoadout = () => {
    const next = loadouts.map((loadout) => ({
      ...loadout,
      active:
        loadout.id === selected.id
          ? true
          : scopeKey(loadout) === scopeKey(selected)
            ? false
            : loadout.active,
    }));
    if (nativeDocument) {
      setNotice("Requesting activation in protected native state…");
      nativeActivate(selected.id)
        .then((snapshot) =>
          acceptNativeSnapshot(
            snapshot,
            "Activated for the next turn. Any in-flight turn keeps its immutable route snapshot.",
          ),
        )
        .catch(() => nativeFailure("Loadout activation"));
      return;
    }
    commit(
      next,
      "Activated for the next turn. Any turn already listening, transcribing, or responding keeps its original route.",
    );
  };

  const acknowledgePrivateEvaluation = () => {
    if (
      !nativeDocument ||
      !privateEvaluationPolicy?.namespaceEligible ||
      !privateEvaluationConsentChecked
    )
      return;
    if (!privateEvaluationArmed) {
      setPrivateEvaluationArmed(true);
      setNotice(
        "Confirm NVIDIA Magpie private-evaluation-only terms. This acknowledgement cannot promote or publish the route and does not prove credentials, stock-voice membership, or provider delivery.",
      );
      return;
    }
    setPrivateEvaluationBusy(true);
    nativeAcknowledgePrivateEvaluation(privateEvaluationPolicy.termsRevision)
      .then((acknowledgement) => {
        const matchesPolicy =
          acknowledgement.providerId === privateEvaluationPolicy.providerId &&
          acknowledgement.termsRevision ===
            privateEvaluationPolicy.termsRevision &&
          acknowledgement.catalogRevision ===
            privateEvaluationPolicy.catalogRevision &&
          acknowledgement.applicationNamespace ===
            privateEvaluationPolicy.applicationNamespace;
        setPrivateEvaluationAcknowledgement(
          matchesPolicy ? acknowledgement : null,
        );
        setPrivateEvaluationArmed(false);
        setPrivateEvaluationConsentChecked(false);
        setNotice(
          matchesPolicy
            ? `Private-evaluation acknowledgement persisted for ${acknowledgement.applicationNamespace}, terms ${acknowledgement.termsRevision}, catalog revision ${acknowledgement.catalogRevision}. Promotion and publication remain unsupported.`
            : "Native acknowledgement did not match the displayed terms, catalog, and application namespace. Activation remains blocked; review the current policy and acknowledge again.",
        );
      })
      .catch(() => nativeFailure("Private-evaluation acknowledgement"))
      .finally(() => setPrivateEvaluationBusy(false));
  };

  const reviewActiveRoute = () => {
    if (!nativeDocument) return;
    setReviewBusy(true);
    setReview(null);
    setNotice(
      "Resolving the active native route without contacting providers…",
    );
    nativeReview(selected, reviewOffline)
      .then((result) => {
        setReview(result);
        setNotice(
          `Native review resolved ${result.resolved.leaf_loadout_id}. Credentials were not checked and no provider request was made.`,
        );
      })
      .catch(() => nativeFailure("Active route review"))
      .finally(() => setReviewBusy(false));
  };

  const deactivateScope = () => {
    if (!nativeDocument || selected.scope === "global" || !activeForScope)
      return;
    setNotice("Returning this scope to its inherited native route…");
    nativeDeactivateScope(activeForScope)
      .then((snapshot) =>
        acceptNativeSnapshot(
          snapshot,
          "Scope override deactivated. The next turn inherits its parent route; any in-flight turn is unchanged.",
        ),
      )
      .catch(() => nativeFailure("Scope deactivation"));
  };

  const changeRoute = (
    role: ProviderRole,
    part: "providerId" | "modelId",
    value: string,
  ) => {
    if (
      role === "tts" &&
      part === "providerId" &&
      value === "nvidia-nim-magpie" &&
      (stockVoices?.status !== "available" || stockVoices.voices.length === 0)
    ) {
      setNotice(
        "Discover an unexpired NVIDIA stock-voice catalog before selecting Magpie.",
      );
      return;
    }
    updateSelected((loadout) => {
      const current = loadout.routes[role];
      const route =
        part === "providerId"
          ? {
              providerId: value,
              modelId: providerFor(role, value).models[0].id,
              ...(role === "tts"
                ? {
                    voiceId:
                      value === "nvidia-nim-magpie"
                        ? stockVoices?.voices[0]?.voiceId
                        : qualifiedStockVoiceId(value),
                  }
                : {}),
            }
          : { ...current, modelId: value };
      loadout.routes[role] = route;
      if (loadout.fallbacks[role]?.providerId === route.providerId) {
        delete loadout.fallbacks[role];
      }
      return loadout;
    }, `${ROLE_META[role].label} preference updated. The change begins on the next turn after activation.`);
  };

  const refreshStockVoices = () => {
    if (!nativeDocument) return;
    setStockVoicesBusy(true);
    setStockVoicesError(null);
    setNotice("Refreshing authenticated NVIDIA Magpie stock voices…");
    nativeDiscoverTtsStockVoices(true)
      .then((result) => {
        setStockVoices(result);
        setStockVoicesError(result.error?.detail ?? null);
        setNotice(
          result.status === "available"
            ? `${result.voices.length} authenticated NVIDIA stock voice${result.voices.length === 1 ? "" : "s"} available for an isolated private-evaluation namespace until ${result.refresh.expiresAtEpochMs ? new Date(result.refresh.expiresAtEpochMs).toLocaleString() : "the provider expiry"}. Promotion and publication remain unsupported.`
            : `NVIDIA stock voices unavailable: ${result.error?.detail ?? "the provider returned no eligible catalog"}`,
        );
      })
      .catch((error: unknown) => {
        const detail =
          error instanceof Error
            ? error.message
            : "NVIDIA stock voices could not be discovered.";
        setStockVoicesError(detail);
        setNotice(`NVIDIA stock voices unavailable: ${detail}`);
      })
      .finally(() => setStockVoicesBusy(false));
  };

  const changeVoiceId = (voiceId: string) => {
    updateSelected(
      (loadout) => {
        loadout.routes.tts = {
          ...loadout.routes.tts,
          voiceId: voiceId.trim() || undefined,
        };
        return loadout;
      },
      "Stock voice identifier updated locally. The change begins on the next turn after activation.",
      false,
    );
  };

  const toggleFallback = (role: ProviderRole, authorized: boolean) => {
    updateSelected(
      (loadout) => {
        loadout.fallbacks[role] = {
          ...(loadout.fallbacks[role] ??
            defaultFallback(role, loadout.routes[role])),
          authorized,
        };
        return loadout;
      },
      authorized
        ? `${ROLE_META[role].short} manual retry authorized. It will still require your action after a failure.`
        : `${ROLE_META[role].short} manual retry removed.`,
    );
  };

  const changeFallback = (role: ProviderRole, providerId: string) => {
    updateSelected((loadout) => {
      const provider = providerFor(role, providerId);
      loadout.fallbacks[role] = {
        providerId,
        modelId: provider.models[0].id,
        authorized: loadout.fallbacks[role]?.authorized ?? false,
      };
      return loadout;
    }, `${ROLE_META[role].short} manual retry preference updated.`);
  };

  const renderRouteDetails = () => {
    const route = selected.routes[activeRole];
    const provider = providerFor(activeRole, route.providerId);
    const model = modelFor(activeRole, route);
    return (
      <details className="loadout-role__disclosure" open>
        <summary>Privacy, cost, and route details</summary>
        <dl className="loadout-role__facts">
          <div>
            <dt>EGRESS</dt>
            <dd>{provider.egress}</dd>
          </div>
          <div>
            <dt>COST</dt>
            <dd>{provider.cost}</dd>
          </div>
          <div>
            <dt>PRIVACY</dt>
            <dd>{provider.privacy}</dd>
          </div>
        </dl>
        {model.note && (
          <p className="loadout-role__note">
            <Icon name="help" size={14} />
            {model.note}
          </p>
        )}
        {provider.note && (
          <p className="loadout-role__note">
            <Icon name="help" size={14} />
            {provider.note}
          </p>
        )}
        {provider.id === "local-visual-worker" && (
          <p className="loadout-role__note">
            <Icon name="warning" size={14} />
            No qualified pack is installed, downloadable, or available for
            activation in this build.
          </p>
        )}
      </details>
    );
  };

  const scopeControls = (
    <div className="loadout-scope-trace" aria-label="Loadout inheritance order">
      {(["global", "game", "character"] as LoadoutScope[]).map(
        (scope, index) => (
          <Button
            key={scope}
            className={`loadout-scope-trace__step ${scopeFilter === scope ? "is-selected" : ""}`}
            onPress={() => chooseScope(scope)}
            aria-pressed={scopeFilter === scope}
          >
            <span>{scopeCopy[scope].eyebrow}</span>
            <strong>{scopeCopy[scope].label}</strong>
            <small>{scopeCopy[scope].description}</small>
            {index < 2 && <Icon name="chevron" aria-hidden="true" />}
          </Button>
        ),
      )}
    </div>
  );

  return (
    <section
      className={`loadout-console loadout-console--${mode}`}
      aria-labelledby="loadout-title"
    >
      <header className="loadout-console__header">
        <div>
          <span className="eyebrow">NEURAL LOADOUT</span>
          <h3 id="loadout-title">
            {mode === "onboarding"
              ? "Choose how this character listens and answers."
              : "Choose the route for the next conversation."}
          </h3>
          <p>
            Select a system on the rig, then slot in its provider and model.
            Your active loadout takes effect when the next conversation begins.
          </p>
        </div>
        <div
          className="loadout-console__turn-lock"
          aria-label="Turn routing safety"
        >
          <Icon name="shield" size={20} />
          <span>ROUTE STATUS</span>
          <strong>Swaps begin next turn</strong>
          <small>
            {selected.active ? "This loadout is active" : "Draft loadout"}
          </small>
        </div>
      </header>

      <div className="loadout-toolbar">
        <div className="loadout-toolbar__identity">
          <span>LOADOUT</span>
          <strong>{selected.name}</strong>
          <small>{targetLabelFor(selected)}</small>
        </div>
        <div className="loadout-toolbar__actions">
          <Button onPress={() => setManagerOpen(true)}>Manage loadouts</Button>
          <Button onPress={() => setAdvancedOpen(true)}>
            Advanced routing
          </Button>
        </div>
      </div>
      {scopeControls}
      <div className="loadout-console__workspace">
        <div className="loadout-editor">
          <div className="loadout-neural-bay">
            <CyberwareAnatomy
              activeRole={activeRole}
              routes={selected.routes}
              isAvailable={routeIsExecutable}
              onSelect={setActiveRole}
            />
            <div className="loadout-roles" aria-label="Provider routes by role">
              {[activeRole].map((role) => {
                const route = selected.routes[role];
                const provider = providerFor(role, route.providerId);
                const model = modelFor(role, route);
                const routeAvailable = routeIsExecutable(role, provider.id);
                const stockVoicePreset =
                  role === "tts" ? stockVoicePresetFor(provider.id) : null;
                const usesCustomVoice = Boolean(
                  role === "tts" &&
                    route.voiceId &&
                    stockVoicePreset &&
                    route.voiceId !== stockVoicePreset.voiceId,
                );
                return (
                  <article
                    className={`loadout-role loadout-role--${provider.execution.toLowerCase()}`}
                    key={role}
                  >
                    <header>
                      <span className="loadout-role__code">
                        {ROLE_META[role].short}
                      </span>
                      <div>
                        <h4>{ROLE_META[role].label}</h4>
                        <p>{ROLE_META[role].description}</p>
                      </div>
                      <StatusPill
                        tone={
                          !routeAvailable
                            ? "warn"
                            : provider.execution === "Cloud"
                              ? "purple"
                              : provider.execution === "Local"
                                ? "teal"
                                : "neutral"
                        }
                      >
                        {routeAvailable ? provider.execution : "Unavailable"}
                      </StatusPill>
                    </header>
                    <div className="loadout-role__selectors">
                      <label>
                        <span>PROVIDER</span>
                        <select
                          aria-label={`${ROLE_META[role].label} provider`}
                          title={provider.name}
                          value={provider.id}
                          onChange={(event) => {
                            if (role === "tts") setCustomVoiceEditing(false);
                            changeRoute(role, "providerId", event.target.value);
                          }}
                        >
                          {ROUTE_OPTIONS[role]
                            .filter(
                              (option) =>
                                (routeIsExecutable(role, option.id) &&
                                  option.selectable !== false) ||
                                option.id === provider.id,
                            )
                            .map((option) => {
                              const executable = routeIsExecutable(
                                role,
                                option.id,
                              );
                              const magpieNeedsVoice =
                                role === "tts" &&
                                option.id === "nvidia-nim-magpie" &&
                                (stockVoices?.status !== "available" ||
                                  stockVoices.voices.length === 0);
                              return (
                                <option
                                  key={option.id}
                                  value={option.id}
                                  disabled={
                                    !executable ||
                                    option.selectable === false ||
                                    magpieNeedsVoice
                                  }
                                >
                                  {option.name}
                                  {!executable
                                    ? role === "stt"
                                      ? " · not wired to push-to-talk"
                                      : " · no qualified live route"
                                    : option.selectable === false
                                      ? " · qualification pending"
                                      : magpieNeedsVoice
                                        ? " · discover stock voices first"
                                        : ""}
                                </option>
                              );
                            })}
                        </select>
                      </label>
                      <label>
                        <span>MODEL / ENGINE</span>
                        <select
                          aria-label={`${ROLE_META[role].label} model`}
                          title={model.name}
                          value={model.id}
                          disabled={!routeAvailable}
                          onChange={(event) =>
                            changeRoute(role, "modelId", event.target.value)
                          }
                        >
                          {provider.models.map((option) => (
                            <option
                              key={option.id}
                              value={option.id}
                              disabled={option.selectable === false}
                            >
                              {option.name}
                              {option.selectable === false
                                ? " · qualification pending"
                                : ""}
                            </option>
                          ))}
                        </select>
                      </label>
                      {role === "tts" &&
                        route.providerId !== "nvidia-nim-magpie" &&
                        stockVoicePreset && (
                          <div className="loadout-voice-choice">
                            <label>
                              <span>STOCK VOICE</span>
                              <select
                                aria-label="Character voice stock voice"
                                value={
                                  customVoiceEditing || usesCustomVoice
                                    ? "custom"
                                    : route.voiceId === stockVoicePreset.voiceId
                                      ? stockVoicePreset.voiceId
                                      : "default"
                                }
                                onChange={(event) => {
                                  if (event.target.value === "custom") {
                                    setCustomVoiceEditing(true);
                                    return;
                                  }
                                  setCustomVoiceEditing(false);
                                  changeVoiceId(
                                    event.target.value === "default"
                                      ? ""
                                      : event.target.value,
                                  );
                                }}
                              >
                                <option value={stockVoicePreset.voiceId}>
                                  {stockVoicePreset.label}
                                </option>
                                <option value="default">
                                  Provider default
                                </option>
                                <option value="custom">Custom voice…</option>
                              </select>
                            </label>
                            <details
                              className="custom-voice-id"
                              open={customVoiceEditing || usesCustomVoice}
                            >
                              <summary>Custom voice ID</summary>
                              <label>
                                <span>PROVIDER VOICE ID</span>
                                <input
                                  aria-label="Character voice stock voice ID"
                                  value={route.voiceId ?? ""}
                                  placeholder="Paste a provider stock voice ID"
                                  maxLength={128}
                                  onChange={(event) =>
                                    changeVoiceId(event.target.value)
                                  }
                                  onBlur={() => {
                                    if (
                                      !nativeDocument ||
                                      nativeCatalogRevision === null
                                    )
                                      return;
                                    nativeUpdate(
                                      selected,
                                      nativeDocument,
                                      nativeCatalogRevision,
                                    )
                                      .then((snapshot) =>
                                        acceptNativeSnapshot(
                                          snapshot,
                                          "Stock voice saved in protected native state.",
                                        ),
                                      )
                                      .catch(() =>
                                        nativeFailure("Stock voice update"),
                                      );
                                  }}
                                />
                              </label>
                            </details>
                          </div>
                        )}
                      {role === "tts" &&
                        route.providerId === "nvidia-nim-magpie" && (
                          <label>
                            <span>DISCOVERED STOCK VOICE</span>
                            <select
                              aria-label="Character voice discovered stock voice"
                              value={
                                stockVoices?.voices.some(
                                  (voice) => voice.voiceId === route.voiceId,
                                )
                                  ? route.voiceId
                                  : ""
                              }
                              disabled={
                                stockVoicesBusy ||
                                stockVoices?.status !== "available" ||
                                stockVoices.voices.length === 0
                              }
                              onChange={(event) =>
                                updateSelected((loadout) => {
                                  loadout.routes.tts = {
                                    ...loadout.routes.tts,
                                    voiceId: event.target.value,
                                  };
                                  return loadout;
                                }, "Authenticated NVIDIA stock voice saved for the next turn after activation.")
                              }
                            >
                              <option value="" disabled>
                                {stockVoicesBusy
                                  ? "Discovering voices…"
                                  : "Choose an authenticated stock voice"}
                              </option>
                              {stockVoices?.voices.map((voice) => (
                                <option
                                  key={voice.voiceId}
                                  value={voice.voiceId}
                                >
                                  {voice.displayName} · {voice.language}
                                  {voice.styles.length
                                    ? ` · ${voice.styles.join(", ")}`
                                    : ""}
                                </option>
                              ))}
                            </select>
                          </label>
                        )}
                      {provider.execution === "Cloud" &&
                        routeAvailable &&
                        onManageProvider && (
                          <ActionButton
                            variant="outline"
                            icon="shield"
                            onPress={() =>
                              onManageProvider(accountProviderId(provider.id))
                            }
                          >
                            Connect or check {provider.name.split(" · ")[0]}
                          </ActionButton>
                        )}
                    </div>
                    {role === "lipSync" && (
                      <section
                        className="mouth-motion-handoff"
                        aria-label="Game mouth motion setup"
                      >
                        <span className="mouth-motion-handoff__glyph">
                          <Icon name="presence" size={18} />
                        </span>
                        <div>
                          <strong>Game mouth motion</strong>
                          <p>Select the NPC and its mouth pack in Games.</p>
                        </div>
                        {onManageMouthMotion && (
                          <ActionButton
                            variant="outline"
                            icon="chevron"
                            onPress={onManageMouthMotion}
                          >
                            Set up mouth motion
                          </ActionButton>
                        )}
                      </section>
                    )}
                    {!routeAvailable && (
                      <p className="loadout-role__note is-error" role="status">
                        <Icon name="warning" size={14} />
                        {role === "stt"
                          ? "The current push-to-talk path supports AssemblyAI · Universal-3 Pro Streaming. Choose it before testing microphone input."
                          : role === "vision"
                            ? "No vision provider is executed by the current turn runtime. Keep this route off."
                            : role === "lipSync"
                              ? "No qualified live lip-sync pack is installed. Audio and subtitles remain available."
                              : "This catalog route is not executable in the current runtime."}
                      </p>
                    )}
                    {role === "tts" && provider.id === "nvidia-nim-magpie" && (
                      <details className="stock-voice-discovery" open>
                        <summary>
                          <strong>NVIDIA Magpie stock voices</strong>
                          <StatusPill
                            tone={
                              stockVoices?.status === "available"
                                ? "ok"
                                : "neutral"
                            }
                          >
                            {stockVoices?.status === "available"
                              ? `${stockVoices.voices.length} discovered`
                              : nativeDocument
                                ? "Private evaluation"
                                : "Native only"}
                          </StatusPill>
                        </summary>
                        <div className="stock-voice-discovery__body">
                          <ActionButton
                            variant="outline"
                            icon="refresh"
                            onPress={refreshStockVoices}
                            isDisabled={!nativeDocument || stockVoicesBusy}
                          >
                            {stockVoicesBusy
                              ? "Discovering NVIDIA voices…"
                              : "Refresh NVIDIA stock voices"}
                          </ActionButton>
                          <p
                            className={stockVoicesError ? "is-error" : ""}
                            role="status"
                          >
                            {!nativeDocument
                              ? "Installed .debug/.review private-evaluation namespace required. Browser preview and the base production namespace cannot select Magpie."
                              : stockVoicesError
                                ? `Unavailable: ${stockVoicesError}`
                                : stockVoices?.status === "available"
                                  ? `${stockVoices.voices.length} authenticated NVIDIA provider-stock voice${stockVoices.voices.length === 1 ? "" : "s"} · ${stockVoices.refresh.cacheHit ? "authenticated cache" : "authenticated refresh"} · expires ${stockVoices.refresh.expiresAtEpochMs ? new Date(stockVoices.refresh.expiresAtEpochMs).toLocaleString() : "not reported"} · private-evaluation namespace only · promotion/publication unsupported`
                                  : "No authenticated NVIDIA stock-voice catalog is loaded. Only an isolated native .debug/.review private-evaluation namespace can discover or select Magpie voices."}
                          </p>
                        </div>
                      </details>
                    )}
                    {(provider.id === "nvidia-nim" ||
                      provider.id === "nvidia-nim-magpie") && (
                      <section
                        className="private-evaluation-acknowledgement"
                        aria-label="NVIDIA provider private evaluation terms"
                      >
                        <details>
                          <summary>
                            <strong>NVIDIA private evaluation only</strong>
                            <StatusPill
                              tone={privateEvaluationReady ? "ok" : "warn"}
                            >
                              {privateEvaluationReady
                                ? "Acknowledged"
                                : "Acknowledgement required"}
                            </StatusPill>
                          </summary>
                          <p>
                            Review the current trial terms once to use this
                            route in the private review build. Magpie also needs
                            a discovered stock voice.
                          </p>
                          <ul className="private-evaluation-egress">
                            {selected.routes.llm.providerId ===
                              "nvidia-nim" && (
                              <li>
                                LLM: conversation text and derived game context
                                leave this PC.
                              </li>
                            )}
                            {selected.routes.embeddings.providerId ===
                              "nvidia-nim" && (
                              <li>
                                Embeddings: selected memory or lore text leaves
                                this PC.
                              </li>
                            )}
                            {selected.routes.tts.providerId ===
                              "nvidia-nim-magpie" && (
                              <li>
                                Magpie TTS: response text and its
                                audio-generation request leave this PC; stock
                                voices only, with no cloning.
                              </li>
                            )}
                          </ul>
                          {privateEvaluationPolicy ? (
                            <dl className="private-evaluation-policy">
                              <div>
                                <dt>Provider</dt>
                                <dd>{privateEvaluationPolicy.providerId}</dd>
                              </div>
                              <div>
                                <dt>Restriction</dt>
                                <dd>{privateEvaluationPolicy.mode}</dd>
                              </div>
                              <div>
                                <dt>Current terms</dt>
                                <dd>{privateEvaluationPolicy.termsRevision}</dd>
                              </div>
                              <div>
                                <dt>Native namespace</dt>
                                <dd>
                                  {privateEvaluationPolicy.applicationNamespace}{" "}
                                  ·{" "}
                                  {privateEvaluationPolicy.namespaceEligible
                                    ? "eligible for private evaluation"
                                    : "ineligible; acknowledgement blocked"}
                                </dd>
                              </div>
                              <div>
                                <dt>Data egress</dt>
                                <dd>
                                  {selectedNvidiaModalities.join(", ")} → NVIDIA
                                  provider cloud
                                </dd>
                              </div>
                              <div>
                                <dt>Promotion / publication</dt>
                                <dd>false / false</dd>
                              </div>
                              <div>
                                <dt>Access scope</dt>
                                <dd>{privateEvaluationPolicy.accessScope}</dd>
                              </div>
                              <div>
                                <dt>Affected selected modalities</dt>
                                <dd>{selectedNvidiaModalities.join(", ")}</dd>
                              </div>
                              <div>
                                <dt>Provider limits</dt>
                                <dd>{privateEvaluationPolicy.rateLimitNote}</dd>
                              </div>
                              <div>
                                <dt>Prohibited data</dt>
                                <dd>
                                  {privateEvaluationPolicy.prohibitedData.join(
                                    ", ",
                                  )}
                                </dd>
                              </div>
                              <div>
                                <dt>Logging / improvement disclosure</dt>
                                <dd>
                                  security abuse logging{" "}
                                  {privateEvaluationPolicy.securityAbuseLogging
                                    ? "disclosed"
                                    : "not disclosed"}
                                  {" · product improvement collection "}
                                  {privateEvaluationPolicy.productImprovementCollectionDisclosed
                                    ? "disclosed"
                                    : "not disclosed"}
                                </dd>
                              </div>
                            </dl>
                          ) : (
                            <p className="is-error" role="status">
                              {privateEvaluationPolicyError ??
                                "Reading the native private-evaluation policy…"}
                            </p>
                          )}
                          {privateEvaluationPolicy && (
                            <a
                              href={privateEvaluationPolicy.termsUrl}
                              target="_blank"
                              rel="noreferrer"
                            >
                              Open NVIDIA API Trial Terms
                            </a>
                          )}
                          {privateEvaluationAcknowledgement &&
                            !privateEvaluationReady && (
                              <small className="is-error">
                                A prior acknowledgement is stale for the current
                                terms, catalog, or native namespace. Re-review
                                and acknowledge the policy below.
                              </small>
                            )}
                          {privateEvaluationReady &&
                            privateEvaluationAcknowledgement && (
                              <small>
                                {privateEvaluationAcknowledgement.termsRevision}{" "}
                                · catalog{" "}
                                {
                                  privateEvaluationAcknowledgement.catalogRevision
                                }
                                {" · "}
                                nonproduction namespace{" "}
                                {
                                  privateEvaluationAcknowledgement.applicationNamespace
                                }
                                {" · "}
                                {new Date(
                                  privateEvaluationAcknowledgement.acknowledgedAtEpochMs,
                                ).toLocaleString()}
                              </small>
                            )}
                          <label className="private-evaluation-consent">
                            <input
                              type="checkbox"
                              checked={privateEvaluationConsentChecked}
                              disabled={
                                !nativeDocument ||
                                !privateEvaluationPolicy?.namespaceEligible ||
                                privateEvaluationReady ||
                                privateEvaluationBusy
                              }
                              onChange={(event) => {
                                setPrivateEvaluationConsentChecked(
                                  event.target.checked,
                                );
                                setPrivateEvaluationArmed(false);
                              }}
                            />
                            <span>
                              I reviewed the exact current NVIDIA trial terms
                              and accept private-evaluation use and provider
                              cloud data processing.
                            </span>
                          </label>
                          <ActionButton
                            variant="outline"
                            icon="shield"
                            onPress={acknowledgePrivateEvaluation}
                            isDisabled={
                              !nativeDocument ||
                              privateEvaluationBusy ||
                              !privateEvaluationPolicy?.namespaceEligible ||
                              !privateEvaluationConsentChecked ||
                              privateEvaluationReady
                            }
                          >
                            {privateEvaluationBusy
                              ? "Persisting acknowledgement…"
                              : privateEvaluationArmed
                                ? "Confirm private-evaluation-only terms"
                                : privateEvaluationReady
                                  ? "Current terms acknowledged"
                                  : "Review and acknowledge terms"}
                          </ActionButton>
                        </details>
                      </section>
                    )}
                  </article>
                );
              })}
            </div>
          </div>

          <footer className="loadout-editor__footer">
            <div role="status" aria-live="polite">
              <Icon name="shield" size={18} />
              <span>{notice}</span>
            </div>
            <ActionButton
              icon="check"
              onPress={activateLoadout}
              isDisabled={
                selected.active ||
                (selectedNvidiaModalities.length > 0 && !privateEvaluationReady)
              }
            >
              {selected.active
                ? "Active for next turn"
                : "Activate for next turn"}
            </ActionButton>
            {selectedNvidiaModalities.length > 0 && !privateEvaluationReady && (
              <small className="loadout-activation-reason">
                Review the current NVIDIA trial terms before activating this
                private evaluation route.
              </small>
            )}
          </footer>
        </div>
      </div>
      <ModalOverlay
        className="workshop-modal-overlay"
        isOpen={managerOpen}
        onOpenChange={setManagerOpen}
        isDismissable
      >
        <Modal className="workshop-modal">
          <Dialog
            className="workshop-dialog loadout-console"
            aria-label="Manage loadouts"
          >
            <header className="workshop-dialog__header">
              <Heading slot="title">Manage loadouts</Heading>
              <Button
                aria-label="Close loadout manager"
                onPress={() => setManagerOpen(false)}
              >
                Close ×
              </Button>
            </header>
            <div className="workshop-dialog__body">
              {scopeControls}
              <aside
                className="loadout-library"
                aria-label={`${scopeCopy[scopeFilter].label} loadouts`}
              >
                <div className="loadout-library__head">
                  <div>
                    <span>{scopeCopy[scopeFilter].eyebrow}</span>
                    <strong>{scopeCopy[scopeFilter].label} loadouts</strong>
                  </div>
                  <Button
                    className="loadout-add"
                    onPress={addLoadout}
                    isDisabled={!targetForScope(scopeFilter)}
                    aria-label={`Create ${scopeFilter} loadout`}
                  >
                    +
                  </Button>
                </div>
                {!targetForScope(scopeFilter) && (
                  <p className="loadout-library__context-required" role="note">
                    {scopeFilter === "game"
                      ? "Select a game in World to create its override."
                      : "Select a game and character in World to create an override."}
                  </p>
                )}
                <div className="loadout-library__list">
                  {currentScopeLoadouts.length === 0 ? (
                    <div className="loadout-library__empty">
                      <Icon name="models" />
                      <strong>No {scopeFilter} override</strong>
                      <p>
                        {targetForScope(scopeFilter)
                          ? "The inherited route stays active until you create one."
                          : scopeFilter === "game"
                            ? "Select a game in World to create its override."
                            : "Select a game and character in World to create an override."}
                      </p>
                    </div>
                  ) : (
                    currentScopeLoadouts.map((loadout) => (
                      <Button
                        key={loadout.id}
                        className={`loadout-library__item ${selected.id === loadout.id ? "is-selected" : ""}`}
                        onPress={() => chooseLoadout(loadout)}
                        aria-pressed={selected.id === loadout.id}
                      >
                        <span className="loadout-library__signal" />
                        <span>
                          <strong>{loadout.name}</strong>
                          <small>{targetLabelFor(loadout)}</small>
                        </span>
                        {loadout.active ? (
                          <StatusPill tone="ok">Active</StatusPill>
                        ) : (
                          <span className="loadout-library__draft">
                            INACTIVE
                          </span>
                        )}
                      </Button>
                    ))
                  )}
                </div>
                <div className="loadout-library__legend">
                  <span>
                    <i /> Active preference
                  </span>
                  <span>Inactive loadouts never route traffic</span>
                </div>
              </aside>{" "}
              <div className="loadout-editor__identity">
                <label>
                  <span>LOADOUT NAME</span>
                  <input
                    aria-label="Loadout name"
                    value={selected.name}
                    maxLength={64}
                    onChange={(event) =>
                      updateSelected(
                        (loadout) => ({ ...loadout, name: event.target.value }),
                        "Name updated locally.",
                        false,
                      )
                    }
                    onBlur={() => {
                      if (!nativeDocument) return;
                      nativeRename(selected.id, selected.name)
                        .then((snapshot) =>
                          acceptNativeSnapshot(
                            snapshot,
                            "Loadout renamed in protected native state.",
                          ),
                        )
                        .catch(() => nativeFailure("Loadout rename"));
                    }}
                  />
                </label>
                <div className="loadout-editor__target">
                  <span>APPLIES TO</span>
                  <strong>{targetLabelFor(selected)}</strong>
                  <small>
                    {selected.scope === "global"
                      ? "Base route"
                      : `Overrides ${selected.scope === "game" ? "global" : "game and global"} choices`}
                  </small>
                </div>
                <div className="loadout-editor__actions">
                  <ActionButton
                    variant="outline"
                    icon="models"
                    onPress={cloneLoadout}
                  >
                    Clone
                  </ActionButton>
                  <ActionButton
                    variant="danger"
                    icon="close"
                    onPress={deleteLoadout}
                    isDisabled={loadouts.length === 1 || selected.active}
                  >
                    Delete loadout
                  </ActionButton>
                </div>
              </div>
              <details className="loadout-overview-details">
                <summary>Connections & active route</summary>
                <div
                  className="loadout-route-summary"
                  aria-label="Selected route summary"
                >
                  <div>
                    <span>CLOUD EGRESS</span>
                    <strong>
                      {cloudVendors.size} provider
                      {cloudVendors.size === 1 ? "" : "s"}
                    </strong>
                    <small>
                      {cloudVendors.size
                        ? [...cloudVendors].join(" · ")
                        : "No selected cloud route"}
                    </small>
                  </div>
                  <div>
                    <span>LOCAL ROLES</span>
                    <strong>{localRoles.length || "None"}</strong>
                    <small>
                      {localRoles.length
                        ? localRoles
                            .map((role) => ROLE_META[role].short)
                            .join(" · ")
                        : "Cloud/off selections only"}
                    </small>
                  </div>
                  <div>
                    <span>ACTIVE AT SCOPE</span>
                    <strong>{activeForScope?.name ?? "None"}</strong>
                    <small>
                      {selected.active
                        ? "You are editing the active preference"
                        : "This loadout is not routing"}
                    </small>
                  </div>
                  <div>
                    <span>REVISION</span>
                    <strong>
                      R{selected.revision.toString().padStart(2, "0")}
                    </strong>
                    <small>Route IDs only · no credential values</small>
                  </div>
                </div>
              </details>
            </div>
          </Dialog>
        </Modal>
      </ModalOverlay>
      <ModalOverlay
        className="workshop-modal-overlay"
        isOpen={advancedOpen}
        onOpenChange={setAdvancedOpen}
        isDismissable
      >
        <Modal className="workshop-modal">
          <Dialog
            className="workshop-dialog loadout-console"
            aria-label="Advanced routing"
          >
            <header className="workshop-dialog__header">
              <Heading slot="title">Advanced routing</Heading>
              <Button
                aria-label="Close advanced routing"
                onPress={() => setAdvancedOpen(false)}
              >
                Close ×
              </Button>
            </header>
            <div className="workshop-dialog__body">
              {renderRouteDetails()}

              <section
                className="manual-fallbacks"
                aria-labelledby="manual-fallbacks-title"
              >
                <details open>
                  <summary>Manual retry routes · off until authorized</summary>
                  <header>
                    <div>
                      <span className="eyebrow">MANUAL RECOVERY ONLY</span>
                      <h4 id="manual-fallbacks-title">
                        Pre-authorize choices you may retry yourself.
                      </h4>
                    </div>
                    <StatusPill tone="teal">
                      Automatic fallback disabled
                    </StatusPill>
                  </header>
                  <p>
                    Authorization adds a visible retry choice after a failure.
                    It never switches providers on its own, never changes a live
                    turn, and never overrides Offline mode.
                  </p>
                  <div className="manual-fallbacks__grid">
                    {fallbackRoles.map((role) => {
                      const fallback =
                        selected.fallbacks[role] ??
                        defaultFallback(role, selected.routes[role]);
                      const eligibleProviders = ROUTE_OPTIONS[role].filter(
                        (provider) =>
                          provider.id !== selected.routes[role].providerId &&
                          provider.execution !== "Off" &&
                          provider.selectable !== false &&
                          routeIsExecutable(role, provider.id),
                      );
                      return (
                        <div
                          className={fallback.authorized ? "is-authorized" : ""}
                          key={role}
                        >
                          <label className="fallback-authorize">
                            <input
                              type="checkbox"
                              checked={fallback.authorized}
                              onChange={(event) =>
                                toggleFallback(role, event.target.checked)
                              }
                            />
                            <span>
                              <strong>
                                {ROLE_META[role].short} manual retry
                              </strong>
                              <small>
                                {fallback.authorized
                                  ? "Authorized · still requires a click"
                                  : "Not authorized"}
                              </small>
                            </span>
                          </label>
                          <select
                            aria-label={`${ROLE_META[role].label} manual fallback provider`}
                            value={fallback.providerId}
                            onChange={(event) =>
                              changeFallback(role, event.target.value)
                            }
                            disabled={!fallback.authorized}
                          >
                            {eligibleProviders.map((provider) => (
                              <option value={provider.id} key={provider.id}>
                                {provider.name} · {provider.models[0].name}
                              </option>
                            ))}
                          </select>
                        </div>
                      );
                    })}
                  </div>
                </details>
              </section>

              <section
                className="loadout-native-review"
                aria-labelledby="loadout-native-review-title"
              >
                <details open>
                  <summary>
                    Validate active inheritance · no provider contact
                  </summary>
                  <header>
                    <div>
                      <span className="eyebrow">PROTECTED NATIVE STATE</span>
                      <h4 id="loadout-native-review-title">
                        Review the active route for this scope.
                      </h4>
                    </div>
                    <StatusPill tone={nativeDocument ? "ok" : "neutral"}>
                      {nativeDocument ? "Installed app" : "Browser preview"}
                    </StatusPill>
                  </header>
                  <p>
                    Review resolves the active global → game → character
                    inheritance chain. It does not validate an inactive draft,
                    contact a provider, or check credentials.
                  </p>
                  {activeForScope && activeForScope.id !== selected.id && (
                    <p className="loadout-native-review__context">
                      This draft is inactive. Review will resolve active loadout
                      “{activeForScope.name}” for {targetLabelFor(selected)}.
                    </p>
                  )}
                  <div className="loadout-native-review__actions">
                    <label>
                      <input
                        type="checkbox"
                        checked={reviewOffline}
                        onChange={(event) => {
                          setReviewOffline(event.target.checked);
                          setReview(null);
                        }}
                        disabled={!nativeDocument}
                      />
                      <span>
                        <strong>Review as fully local</strong>
                        <small>
                          Hosted routes should fail the native policy check
                        </small>
                      </span>
                    </label>
                    <ActionButton
                      variant="outline"
                      icon="shield"
                      onPress={reviewActiveRoute}
                      isDisabled={!nativeDocument || reviewBusy}
                    >
                      {reviewBusy ? "Reviewing…" : "Review active route"}
                    </ActionButton>
                    {selected.scope !== "global" && (
                      <ActionButton
                        variant="danger"
                        icon="close"
                        onPress={deactivateScope}
                        isDisabled={!nativeDocument || !activeForScope}
                      >
                        Return scope to inherited route
                      </ActionButton>
                    )}
                  </div>
                  {!nativeDocument && (
                    <p className="loadout-native-review__reason">
                      Review and scope deactivation require the installed app.
                      Browser preview changes remain local to this browser.
                    </p>
                  )}
                  {selected.scope !== "global" &&
                    !activeForScope &&
                    nativeDocument && (
                      <p className="loadout-native-review__reason">
                        This scope already inherits its parent route; there is
                        no active override to deactivate.
                      </p>
                    )}
                  {review && (
                    <dl className="loadout-native-review__receipt">
                      <div>
                        <dt>RESOLVED LEAF</dt>
                        <dd>{review.resolved.leaf_loadout_id}</dd>
                      </div>
                      <div>
                        <dt>INHERITANCE</dt>
                        <dd>{review.resolved.inheritance_chain.join(" → ")}</dd>
                      </div>
                      <div>
                        <dt>CONFIGURED ROLES</dt>
                        <dd>{Object.keys(review.resolved.roles).length} / 6</dd>
                      </div>
                      <div>
                        <dt>POLICY</dt>
                        <dd>
                          {review.offline ? "Fully local" : "Online allowed"}
                        </dd>
                      </div>
                      <div>
                        <dt>PROVIDER CONTACT</dt>
                        <dd>
                          {review.networkRequestPerformed
                            ? "Performed"
                            : "None"}
                        </dd>
                      </div>
                      <div>
                        <dt>CREDENTIAL CHECK</dt>
                        <dd>
                          {review.credentialsChecked
                            ? "Checked"
                            : "Not checked"}
                        </dd>
                      </div>
                    </dl>
                  )}
                </details>
              </section>
            </div>
          </Dialog>
        </Modal>
      </ModalOverlay>
    </section>
  );
}
