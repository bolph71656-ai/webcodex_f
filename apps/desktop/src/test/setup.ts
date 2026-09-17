import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

beforeEach(() => {
  // Most UI tests intentionally exercise the existing Simplified Chinese catalog.
  // Production defaults to Japanese; locale-specific tests clear or override this.
  window.localStorage.setItem("webcodex.desktop.locale", "zh-CN");
});

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});
