import { beforeEach, describe, expect, it, vi } from "vitest";

const mount = vi.fn(() => ({ mounted: true }));

vi.mock("svelte", async (importOriginal) => ({
  ...(await importOriginal<typeof import("svelte")>()),
  mount
}));
vi.mock("./App.svelte", () => ({ default: "AppComponent" }));

describe("application entry point", () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="app"></div>';
    mount.mockClear();
  });

  it("mounts the Svelte 5 component into the app root", async () => {
    await import("./main");
    expect(mount).toHaveBeenCalledWith("AppComponent", {
      target: document.getElementById("app")
    });
  });
});
