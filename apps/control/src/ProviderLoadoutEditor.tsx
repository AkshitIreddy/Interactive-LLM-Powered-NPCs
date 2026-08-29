import { useEffect, useMemo, useState } from "react";
import { Button } from "react-aria-components";
import { ActionButton, StatusPill } from "./components";
import { Icon } from "./icons";
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
  nativeClone,
  nativeCreate,
  nativeDelete,
  nativeRename,
  nativeSnapshot,
  nativeUpdate,
  type NativeLoadoutDocument,
  type NativeLoadoutSnapshot,
} from "./providerLoadoutBridge";

const FALLBACK_ROLES: ProviderRole[] = ["llm", "stt", "tts", "retrieval"];

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
      candidate.id !== primary.providerId && candidate.selectable !== false,
  );
  const selected = provider ?? ROUTE_OPTIONS[role][0];
  return {
    providerId: selected.id,
    modelId: selected.models[0].id,
    authorized: false,
  };
}

export function ProviderLoadoutEditor() {
  const [loadouts, setLoadouts] =
    useState<ProviderLoadout[]>(readBrowserLoadouts);
  const [selectedId, setSelectedId] = useState(loadouts[0].id);
  const [scopeFilter, setScopeFilter] = useState<LoadoutScope>(
    loadouts[0].scope,
  );
  const [notice, setNotice] = useState(
    "Route choices are saved as a local preview. No provider request was made.",
  );
  const [nativeDocument, setNativeDocument] =
    useState<NativeLoadoutDocument | null>(null);

  const selected =
    loadouts.find((loadout) => loadout.id === selectedId) ?? loadouts[0];
  const currentScopeLoadouts = loadouts.filter(
    (loadout) => loadout.scope === scopeFilter,
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
    setLoadouts(nativeLoadouts);
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
      .then((snapshot) => {
        if (current && snapshot) acceptNativeSnapshot(snapshot);
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
    commit(next, message);
    const changed = next.find((loadout) => loadout.id === selected.id);
    if (syncNative && changed && nativeDocument) {
      nativeUpdate(changed, nativeDocument)
        .then((snapshot) => acceptNativeSnapshot(snapshot, message))
        .catch(() => nativeFailure("Loadout update"));
    }
  };

  const chooseLoadout = (loadout: ProviderLoadout) => {
    setSelectedId(loadout.id);
    setScopeFilter(loadout.scope);
    setNotice(
      loadout.active
        ? "This is the active preference for its scope. A running turn keeps its original route snapshot."
        : "Reviewing an inactive loadout. Nothing is routed until you activate it.",
    );
  };

  const addLoadout = () => {
    const created = makeLoadout(scopeFilter, loadouts.length + 1);
    commit(
      [...loadouts, created],
      "New loadout created locally. Name it, review every role, then activate it explicitly.",
      created.id,
    );
    if (nativeDocument) {
      nativeCreate(created, nativeDocument)
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
    commit(
      next,
      "Activated for the next turn. Any turn already listening, transcribing, or responding keeps its original route.",
    );
    if (nativeDocument) {
      nativeActivate(selected.id)
        .then((snapshot) =>
          acceptNativeSnapshot(
            snapshot,
            "Activated for the next turn. Any in-flight turn keeps its immutable route snapshot.",
          ),
        )
        .catch(() => nativeFailure("Loadout activation"));
    }
  };

  const changeRoute = (
    role: ProviderRole,
    part: "providerId" | "modelId",
    value: string,
  ) => {
    updateSelected((loadout) => {
      const current = loadout.routes[role];
      const route =
        part === "providerId"
          ? {
              providerId: value,
              modelId: providerFor(role, value).models[0].id,
            }
          : { ...current, modelId: value };
      loadout.routes[role] = route;
      if (loadout.fallbacks[role]?.providerId === route.providerId) {
        delete loadout.fallbacks[role];
      }
      return loadout;
    }, `${ROLE_META[role].label} preference updated. The change begins on the next turn after activation.`);
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

  return (
    <section className="loadout-console" aria-labelledby="loadout-title">
      <header className="loadout-console__header">
        <div>
          <span className="eyebrow">PROVIDER & MODEL LOADOUTS</span>
          <h3 id="loadout-title">Build a route for every kind of turn.</h3>
          <p>
            Keep multiple API/model combinations, switch deliberately, and let
            game or character overrides inherit the rest.
          </p>
        </div>
        <div
          className="loadout-console__turn-lock"
          aria-label="Turn routing safety"
        >
          <Icon name="shield" size={20} />
          <span>TURN SNAPSHOT</span>
          <strong>Swaps begin next turn</strong>
          <small>Never reroutes an in-flight response</small>
        </div>
      </header>

      <div
        className="loadout-scope-trace"
        aria-label="Loadout inheritance order"
      >
        {(["global", "game", "character"] as LoadoutScope[]).map(
          (scope, index) => (
            <Button
              key={scope}
              className={`loadout-scope-trace__step ${scopeFilter === scope ? "is-selected" : ""}`}
              onPress={() => setScopeFilter(scope)}
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

      <div className="loadout-console__workspace">
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
              aria-label={`Create ${scopeFilter} loadout`}
            >
              +
            </Button>
          </div>
          <div className="loadout-library__list">
            {currentScopeLoadouts.length === 0 ? (
              <div className="loadout-library__empty">
                <Icon name="models" />
                <strong>No {scopeFilter} override</strong>
                <p>Inherited routes remain visible until you create one.</p>
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
                    <small>{loadout.targetLabel}</small>
                  </span>
                  {loadout.active ? (
                    <StatusPill tone="ok">Active</StatusPill>
                  ) : (
                    <span className="loadout-library__draft">INACTIVE</span>
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
        </aside>

        <div className="loadout-editor">
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
              <strong>{selected.targetLabel}</strong>
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

          <div
            className="loadout-route-summary"
            aria-label="Selected route summary"
          >
            <div>
              <span>CLOUD EGRESS</span>
              <strong>
                {cloudVendors.size} provider{cloudVendors.size === 1 ? "" : "s"}
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
                  ? localRoles.map((role) => ROLE_META[role].short).join(" · ")
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
              <strong>R{selected.revision.toString().padStart(2, "0")}</strong>
              <small>Route IDs only · no credential values</small>
            </div>
          </div>

          <div className="loadout-roles" aria-label="Provider routes by role">
            {ROLE_ORDER.map((role) => {
              const route = selected.routes[role];
              const provider = providerFor(role, route.providerId);
              const model = modelFor(role, route);
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
                        provider.execution === "Cloud"
                          ? "purple"
                          : provider.execution === "Local"
                            ? "teal"
                            : "neutral"
                      }
                    >
                      {provider.execution}
                    </StatusPill>
                  </header>
                  <div className="loadout-role__selectors">
                    <label>
                      <span>PROVIDER</span>
                      <select
                        aria-label={`${ROLE_META[role].label} provider`}
                        title={provider.name}
                        value={provider.id}
                        onChange={(event) =>
                          changeRoute(role, "providerId", event.target.value)
                        }
                      >
                        {ROUTE_OPTIONS[role].map((option) => (
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
                    <label>
                      <span>MODEL / ENGINE</span>
                      <select
                        aria-label={`${ROLE_META[role].label} model`}
                        title={model.name}
                        value={model.id}
                        onChange={(event) =>
                          changeRoute(role, "modelId", event.target.value)
                        }
                      >
                        {provider.models.map((option) => (
                          <option key={option.id} value={option.id}>
                            {option.name}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
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
                  {provider.note && (
                    <p className="loadout-role__note">
                      <Icon name="help" size={14} />
                      {provider.note}
                    </p>
                  )}
                  {provider.id === "local-visual-worker" && (
                    <p className="loadout-role__note">
                      <Icon name="warning" size={14} />
                      Choose the desired candidate on Models. No pack is
                      downloadable yet.
                    </p>
                  )}
                </article>
              );
            })}
          </div>

          <section
            className="manual-fallbacks"
            aria-labelledby="manual-fallbacks-title"
          >
            <header>
              <div>
                <span className="eyebrow">MANUAL RECOVERY ONLY</span>
                <h4 id="manual-fallbacks-title">
                  Pre-authorize choices you may retry yourself.
                </h4>
              </div>
              <StatusPill tone="teal">Automatic fallback disabled</StatusPill>
            </header>
            <p>
              Authorization adds a visible retry choice after a failure. It
              never switches providers on its own, never changes a live turn,
              and never overrides Offline mode.
            </p>
            <div className="manual-fallbacks__grid">
              {FALLBACK_ROLES.map((role) => {
                const fallback =
                  selected.fallbacks[role] ??
                  defaultFallback(role, selected.routes[role]);
                const eligibleProviders = ROUTE_OPTIONS[role].filter(
                  (provider) =>
                    provider.id !== selected.routes[role].providerId &&
                    provider.selectable !== false,
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
                        <strong>{ROLE_META[role].short} manual retry</strong>
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
          </section>

          <footer className="loadout-editor__footer">
            <div role="status" aria-live="polite">
              <Icon name="shield" size={18} />
              <span>{notice}</span>
            </div>
            <ActionButton
              icon="check"
              onPress={activateLoadout}
              isDisabled={selected.active}
            >
              {selected.active
                ? "Active for next turn"
                : "Activate for next turn"}
            </ActionButton>
          </footer>
        </div>
      </div>
    </section>
  );
}
