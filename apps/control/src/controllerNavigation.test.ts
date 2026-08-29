import { describe, expect, it, vi } from "vitest";
import {
  processControllerFrame,
  type ControllerFrameState,
} from "./controllerNavigation";

const gamepad = (buttons: number[] = [], axes = [0, 0]) => ({
  buttons: Array.from({ length: 16 }, (_, index) => ({
    pressed: buttons.includes(index),
  })) as GamepadButton[],
  axes,
});

describe("experimental controller navigation", () => {
  it("moves focus with D-pad and activates with A on an edge", () => {
    document.body.innerHTML =
      "<button>One</button><button>Two</button><button>Three</button>";
    const buttons = Array.from(document.querySelectorAll("button"));
    const clicked = vi.fn();
    buttons[1].addEventListener("click", clicked);
    buttons[0].focus();
    let state: ControllerFrameState = {
      lastMoveAt: -Infinity,
      aPressed: false,
      bPressed: false,
    };
    state = processControllerFrame(gamepad([15]), 1000, state);
    expect(document.activeElement).toBe(buttons[1]);
    state = processControllerFrame(gamepad(), 1010, state);
    processControllerFrame(gamepad([0]), 1020, state);
    expect(clicked).toHaveBeenCalledOnce();
  });

  it("uses B only for an explicit visible Back control", () => {
    document.body.innerHTML = "<button>Continue</button><button>Back</button>";
    const back = document.querySelectorAll("button")[1];
    const clicked = vi.fn();
    back.addEventListener("click", clicked);
    processControllerFrame(gamepad([1]), 1000, {
      lastMoveAt: -Infinity,
      aPressed: false,
      bPressed: false,
    });
    expect(clicked).toHaveBeenCalledOnce();
  });
});
