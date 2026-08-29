import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

Object.defineProperty(window, "matchMedia", {
  writable: true,
  value: (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => undefined,
    removeListener: () => undefined,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    dispatchEvent: () => false,
  }),
});

class ResizeObserverMock {
  observe() {
    /* deterministic test no-op */
  }
  unobserve() {
    /* deterministic test no-op */
  }
  disconnect() {
    /* deterministic test no-op */
  }
}

Object.defineProperty(window, "ResizeObserver", {
  writable: true,
  value: ResizeObserverMock,
});

beforeEach(() => {
  window.history.replaceState(null, "", "/");
  document.documentElement.dataset.contrast = "normal";
  document.documentElement.dataset.theme = "dark";
  document.documentElement.dataset.largeText = "false";
  document.documentElement.classList.remove("force-reduced-motion");
});

afterEach(() => cleanup());
