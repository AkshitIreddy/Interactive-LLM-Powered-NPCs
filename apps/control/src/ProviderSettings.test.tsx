import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { ProviderSettings } from "./ProviderSettings";
import {
  getLastBrowserProviderCommandForTests,
  resetBrowserProviderFixtureForTests,
} from "./providerCredentials";

describe("provider credential boundary", () => {
  beforeEach(() => {
    resetBrowserProviderFixtureForTests();
    window.history.replaceState(null, "", "/");
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  it("opens the native prompt with provider ID only and exposes no credential input", async () => {
    const user = userEvent.setup();
    render(<ProviderSettings />);
    await user.click(screen.getByRole("button", { name: "Manage loadouts" }));
    expect(
      screen.getByRole("textbox", { name: "Loadout name" }),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Close loadout manager" }),
    );
    expect(document.querySelector('input[type="password"]')).toBeNull();
    expect(screen.queryByLabelText("API credential")).not.toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Open secure Windows key prompt" }),
    );
    await waitFor(() =>
      expect(screen.getByText(/presence status only/i)).toBeInTheDocument(),
    );
    expect(getLastBrowserProviderCommandForTests()).toEqual({
      command: "prompt_and_save_provider_credential",
      args: { providerId: "openai" },
    });
  });

  it("shows local providers without a credential field", async () => {
    const user = userEvent.setup();
    render(<ProviderSettings />);
    await user.click(screen.getByRole("button", { name: /Moonshine local/ }));
    expect(
      screen.getByText("Advanced local route unavailable in this preview."),
    ).toBeInTheDocument();
    expect(screen.getByText("No key needed")).toBeInTheDocument();
    expect(screen.queryByLabelText("API credential")).not.toBeInTheDocument();
  });

  it("routes NVIDIA NIM through the provider-ID-only native prompt", async () => {
    const user = userEvent.setup();
    render(<ProviderSettings />);
    await user.click(screen.getByRole("button", { name: /NVIDIA NIM/ }));
    expect(
      screen.getByText(/Stock Magpie voice discovery is available/),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/ASR remains pending gRPC endpoint qualification/),
    ).toBeInTheDocument();
    await user.click(
      screen.getByRole("button", { name: "Open secure Windows key prompt" }),
    );
    await waitFor(() =>
      expect(getLastBrowserProviderCommandForTests()).toEqual({
        command: "prompt_and_save_provider_credential",
        args: { providerId: "nvidia-nim" },
      }),
    );
    expect(document.querySelector('input[type="password"]')).toBeNull();
  });

  it("honors an explicit NVIDIA URL selection without contacting it", () => {
    window.history.replaceState(
      null,
      "",
      "/?page=settings&settingsSection=providers&provider=nvidia-nim",
    );
    render(<ProviderSettings />);

    expect(screen.getByRole("button", { name: /NVIDIA NIM/ })).toHaveClass(
      "is-selected",
    );
    expect(screen.getByTestId("provider-public-state")).toHaveTextContent(
      '"selectedProviderId":"nvidia-nim"',
    );
    expect(getLastBrowserProviderCommandForTests()).toBeNull();
  });

  it("tests and deletes only an existing saved reference", async () => {
    const user = userEvent.setup();
    render(<ProviderSettings />);
    await waitFor(() =>
      expect(screen.getByText("Saved reference reported")).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "Check setup" }));
    await waitFor(() =>
      expect(
        screen.getByText(/Setup check passed:.*No provider request was made/i),
      ).toBeInTheDocument(),
    );
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(
        screen.getByText(/Credential removed from the Windows vault/i),
      ).toBeInTheDocument(),
    );
    expect(screen.getByText("Not connected")).toBeInTheDocument();
  });
});
