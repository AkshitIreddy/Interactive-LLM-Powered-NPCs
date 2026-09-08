import { Button } from "react-aria-components";
import {
  modelFor,
  providerFor,
  ROLE_META,
  ROLE_ORDER,
  type ProviderLoadout,
  type ProviderRole,
} from "./providerLoadouts";

interface CyberwareAnatomyProps {
  activeRole: ProviderRole;
  routes: ProviderLoadout["routes"];
  isAvailable: (role: ProviderRole, providerId: string) => boolean;
  onSelect: (role: ProviderRole) => void;
}

const anatomyLabels: Record<ProviderRole, string> = {
  llm: "Cognition",
  stt: "Hearing",
  tts: "Voice",
  embeddings: "Memory",
  vision: "Sight",
  lipSync: "Mouth sync",
};

export function CyberwareAnatomy({
  activeRole,
  routes,
  isAvailable,
  onSelect,
}: CyberwareAnatomyProps) {
  return (
    <div className="cyberware-anatomy" aria-label="Conversation system map">
      <div className="cyberware-anatomy__field" aria-hidden="true">
        <span />
        <span />
        <span />
      </div>
      <img
        className="cyberware-anatomy__figure"
        src="/art/neural-anatomy.png"
        alt=""
        aria-hidden="true"
      />
      <div className="cyberware-anatomy__core" aria-hidden="true">
        <span />
      </div>
      {ROLE_ORDER.map((role) => {
        const route = routes[role];
        const provider = providerFor(role, route.providerId);
        const model = modelFor(role, route);
        const available = isAvailable(role, provider.id);
        const mouthMotionUsesGameSetup =
          role === "lipSync" && provider.id === "disabled";
        return (
          <Button
            key={role}
            className={`cyberware-node cyberware-node--${role} ${
              activeRole === role ? "is-selected" : ""
            } ${available ? "" : "is-unavailable"}`}
            aria-pressed={activeRole === role}
            aria-label={`${ROLE_META[role].label}. ${provider.name}. ${model.name}. ${
              available ? provider.execution : "Unavailable"
            }`}
            onPress={() => onSelect(role)}
          >
            <span className="cyberware-node__pin" aria-hidden="true">
              <i />
            </span>
            <span className="cyberware-node__content">
              <span className="cyberware-node__index">
                {String(ROLE_ORDER.indexOf(role) + 1).padStart(2, "0")}
              </span>
              <span>
                <strong>{anatomyLabels[role]}</strong>
                <small>
                  {mouthMotionUsesGameSetup
                    ? "Games setup · optional model Off"
                    : `${provider.name} · ${model.name}`}
                </small>
              </span>
              <em>
                {mouthMotionUsesGameSetup
                  ? "Game setup"
                  : available
                    ? provider.execution
                    : "Setup needed"}
              </em>
            </span>
          </Button>
        );
      })}
      <div className="cyberware-anatomy__caption" aria-hidden="true">
        <span>NEURAL ROUTE</span>
        <strong>{anatomyLabels[activeRole]}</strong>
      </div>
    </div>
  );
}
