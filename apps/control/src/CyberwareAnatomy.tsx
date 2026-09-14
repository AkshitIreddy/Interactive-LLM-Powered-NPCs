import { useLayoutEffect, useRef, useState } from "react";
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
    point: [0.52, 0.28],
  },
  {
    role: "embeddings",
    label: "Memory",
    purpose: "Recall relevant context",
    point: [0.2, 0.34],
  },
  {
    role: "stt",
    label: "Hearing",
    purpose: "Understand your speech",
    point: [0.34, 0.43],
  },
  {
    role: "vision",
    label: "Sight",
    purpose: "Read the scene",
    point: [0.69, 0.43],
  },
  {
    role: "tts",
    label: "Voice",
    purpose: "Speak the reply",
    point: [0.78, 0.59],
  },
  {
    role: "lipSync",
    label: "Mouth sync",
    purpose: "Animate the response",
    point: [0.78, 0.65],
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
  const canvasRef = useRef<HTMLDivElement>(null);
  const figureRef = useRef<HTMLImageElement>(null);
  const [wire, setWire] = useState({
    width: 560,
    height: 500,
    fromX: 180,
    fromY: 80,
    x: 400,
    y: 140,
  });
  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    const figure = figureRef.current;
    if (!canvas || !figure) return;
    const update = () => {
      const button = canvas.querySelector(".neural-system.is-selected");
      if (!button) return;
      const box = canvas.getBoundingClientRect();
      const art = figure.getBoundingClientRect();
      const source = button.getBoundingClientRect();
      const scale = Math.min(
        art.width / (figure.naturalWidth || 1024),
        art.height / (figure.naturalHeight || 1536),
      );
      const width = (figure.naturalWidth || 1024) * scale;
      const height = (figure.naturalHeight || 1536) * scale;
      setWire({
        width: box.width,
        height: box.height,
        fromX: source.right - box.left,
        fromY: source.top - box.top + source.height / 2,
        x:
          art.left -
          box.left +
          (art.width - width) / 2 +
          width * active.point[0],
        y:
          art.top -
          box.top +
          (art.height - height) / 2 +
          height * active.point[1],
      });
    };
    update();
    figure.addEventListener("load", update);
    const observer =
      typeof ResizeObserver !== "undefined" ? new ResizeObserver(update) : null;
    observer?.observe(canvas);
    return () => {
      observer?.disconnect();
      figure.removeEventListener("load", update);
    };
  }, [activeRole, active]);
  return (
    <div className="neural-map" aria-label="Conversation system map">
      <div className="neural-map__heading">
        <span>SYSTEM MAP</span>
        <small>Select a capability to configure</small>
      </div>
      <div className="neural-map__canvas" ref={canvasRef}>
        <img
          className="neural-map__figure"
          ref={figureRef}
          src="/art/neural-interface-v2.png"
          alt="Synthetic intelligence head with brain circuitry, memory modules, hearing receiver, optical sensor and articulated mouth"
        />
        <svg
          className="neural-map__links"
          viewBox={`0 0 ${wire.width || 560} ${wire.height || 500}`}
          aria-hidden="true"
          preserveAspectRatio="none"
        >
          <path
            d={`M ${wire.fromX} ${wire.fromY} H ${wire.fromX + 22} L ${wire.x - 14} ${wire.y} H ${wire.x}`}
          />
          <circle cx={wire.x} cy={wire.y} r="9" />
          <circle cx={wire.x} cy={wire.y} r="2.5" />
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
