import { useEffect } from "react";

export interface ControllerFrameState {
  lastMoveAt: number;
  aPressed: boolean;
  bPressed: boolean;
}
const selector =
  'button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])';

const focusables = () =>
  Array.from(document.querySelectorAll<HTMLElement>(selector)).filter(
    (element) =>
      !element.hidden && element.getAttribute("aria-hidden") !== "true",
  );

export function processControllerFrame(
  gamepad: Pick<Gamepad, "buttons" | "axes">,
  now: number,
  state: ControllerFrameState,
): ControllerFrameState {
  const horizontal = gamepad.axes[0] ?? 0;
  const vertical = gamepad.axes[1] ?? 0;
  const previous = Boolean(
    gamepad.buttons[14]?.pressed ||
      gamepad.buttons[12]?.pressed ||
      horizontal < -0.65 ||
      vertical < -0.65,
  );
  const next = Boolean(
    gamepad.buttons[15]?.pressed ||
      gamepad.buttons[13]?.pressed ||
      horizontal > 0.65 ||
      vertical > 0.65,
  );
  if ((previous || next) && now - state.lastMoveAt >= 240) {
    const items = focusables();
    if (items.length) {
      const current = items.indexOf(document.activeElement as HTMLElement);
      const delta = previous ? -1 : 1;
      items[
        current < 0
          ? delta > 0
            ? 0
            : items.length - 1
          : (current + delta + items.length) % items.length
      ]?.focus();
    }
    state.lastMoveAt = now;
  }
  const aPressed = Boolean(gamepad.buttons[0]?.pressed);
  if (
    aPressed &&
    !state.aPressed &&
    document.activeElement instanceof HTMLElement
  )
    document.activeElement.click();
  const bPressed = Boolean(gamepad.buttons[1]?.pressed);
  if (bPressed && !state.bPressed) {
    const candidates = focusables();
    const back = candidates.find(
      (element) =>
        /^(back|close|cancel|finish later)$/i.test(
          element.textContent?.trim() ?? "",
        ) || /close/i.test(element.getAttribute("aria-label") ?? ""),
    );
    back?.click();
  }
  return { lastMoveAt: state.lastMoveAt, aPressed, bPressed };
}

export function useExperimentalControllerNavigation() {
  useEffect(() => {
    if (!navigator.getGamepads) return;
    let frame = 0;
    let state: ControllerFrameState = {
      lastMoveAt: -Infinity,
      aPressed: false,
      bPressed: false,
    };
    const poll = (now: number) => {
      if (document.visibilityState === "visible") {
        const gamepad = Array.from(navigator.getGamepads()).find(Boolean);
        if (gamepad) state = processControllerFrame(gamepad, now, state);
      }
      frame = requestAnimationFrame(poll);
    };
    frame = requestAnimationFrame(poll);
    return () => cancelAnimationFrame(frame);
  }, []);
}
