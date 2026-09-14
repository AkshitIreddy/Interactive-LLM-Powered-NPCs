import { Button } from "react-aria-components";
import {
  modelFor,
  providerFor,
  ROLE_META,
  type ProviderLoadout,
  type ProviderRole,
} from "./providerLoadouts";

interface CyberwareAnatomyProps {
  activeRole: ProviderRole;
  routes: ProviderLoadout["routes"];
  isAvailable: (role: ProviderRole, providerId: string) => boolean;
  onSelect: (role: ProviderRole) => void;
}

const systems: {
  role: ProviderRole;
  label: string;
  purpose: string;
  point: [number, number];
}[] = [
  {
    role: "llm",
    label: "Cognition",
    purpose: "Compose the reply",
    point: [404, 137],
  },
  {
    role: "embeddings",
    label: "Memory",
    purpose: "Recall relevant context",
    point: [293, 193],
  },
  {
    role: "stt",
    label: "Hearing",
    purpose: "Understand your speech",
    point: [347, 269],
  },
  {
    role: "vision",
    label: "Sight",
    purpose: "Read the scene",
    point: [451, 268],
  },
  {
    role: "tts",
    label: "Voice",
    purpose: "Speak the reply",
    point: [489, 353],
  },
  {
    role: "lipSync",
    label: "Mouth sync",
    purpose: "Animate the response",
    point: [482, 372],
  },
];

export function CyberwareAnatomy({
  activeRole,
  routes,
  isAvailable,
  onSelect,
}: CyberwareAnatomyProps) {
  const activeIndex = systems.findIndex((system) => system.role === activeRole);
  const active = systems[activeIndex];
  const [x, y] = active.point;
  const rowY = 100 + activeIndex * 65;
  return (
    <div className="neural-map" aria-label="Conversation system map">
      <div className="neural-map__heading">
        <span>SYSTEM MAP</span>
        <small>Select a capability to configure</small>
      </div>
      <div className="neural-map__canvas">
        <img
          className="neural-map__figure"
          src="/art/neural-interface-v2.png"
          alt="Synthetic intelligence head with brain circuitry, memory modules, hearing receiver, optical sensor and articulated mouth"
        />
        <svg
          className="neural-map__links"
          viewBox="0 0 560 500"
          aria-hidden="true"
          preserveAspectRatio="none"
        >
          <path d={`M 183 ${rowY} H 219 L ${x - 22} ${y} H ${x}`} />
          <circle cx={x} cy={y} r="9" />
          <circle cx={x} cy={y} r="2.5" />
        </svg>
        <div className="neural-map__systems">
          {systems.map(({ role, label, purpose }, index) => {
            const route = routes[role];
            const provider = providerFor(role, route.providerId);
            const model = modelFor(role, route);
            const available = isAvailable(role, provider.id);
            const gameSetup = role === "lipSync" && provider.id === "disabled";
            return (
              <Button
                key={role}
                className={`neural-system ${activeRole === role ? "is-selected" : ""}`}
                aria-pressed={activeRole === role}
                aria-label={`${ROLE_META[role].label}. ${provider.name}. ${model.name}. ${available ? provider.execution : "Unavailable"}`}
                onPress={() => onSelect(role)}
              >
                <span className="neural-system__number">
                  {String(index + 1).padStart(2, "0")}
                </span>
                <span className="neural-system__copy">
                  <strong>{label}</strong>
                  <small>{purpose}</small>
                  <em>{gameSetup ? "Set up in Games" : provider.name}</em>
                </span>
                <span
                  className={`neural-system__status ${available && provider.execution !== "Off" ? "is-ready" : ""}`}
                  aria-hidden="true"
                />
              </Button>
            );
          })}
        </div>
        <div className="neural-map__caption" aria-hidden="true">
          <span>SELECTED SYSTEM</span>
          <strong>{active.label}</strong>
        </div>
      </div>
    </div>
  );
}
