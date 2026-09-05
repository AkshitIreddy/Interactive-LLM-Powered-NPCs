import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ProviderAccounts } from "./ProviderAccounts";

const accounts = [
  {
    providerId: "assemblyai",
    displayName: "AssemblyAI",
    credentialReference: "providers/assemblyai",
    status: "present" as const,
    detail: "Native bootstrap reports a saved AssemblyAI key reference.",
  },
];

describe("provider accounts", () => {
  it("offers the fixed named Mistral and OpenRouter vault targets", () => {
    render(
      <ProviderAccounts
        accounts={[]}
        nativeAvailable
        busy={null}
        onAction={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: "Mistral AI" })).toHaveValue(
      "mistral",
    );
    expect(screen.getByRole("option", { name: "OpenRouter" })).toHaveValue(
      "openrouter",
    );
  });

  it("does not turn supplied rows into credential presence outside the native app", async () => {
    const user = userEvent.setup();
    render(
      <ProviderAccounts
        accounts={accounts}
        nativeAvailable={false}
        busy={null}
        onAction={vi.fn()}
      />,
    );

    await user.selectOptions(
      screen.getByRole("combobox", { name: "Provider account" }),
      "assemblyai",
    );
    expect(screen.getByText("Desktop app required")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Connect account" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "Check saved key" }),
    ).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: "Remove key" }),
    ).not.toBeInTheDocument();
  });

  it("makes every hosted account generic, including bootstrap-backed AssemblyAI", async () => {
    const user = userEvent.setup();
    const onProviderChange = vi.fn();
    render(
      <ProviderAccounts
        accounts={accounts}
        nativeAvailable
        busy={null}
        onAction={vi.fn()}
        onProviderChange={onProviderChange}
      />,
    );

    const selector = screen.getByRole("combobox", { name: "Provider account" });
    await user.selectOptions(selector, "assemblyai");
    expect(onProviderChange).toHaveBeenCalledWith("assemblyai");
    expect(screen.getByText("Connected key")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Native bootstrap reports a saved AssemblyAI key reference.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Replace key" })).toBeEnabled();
    expect(
      screen.getByRole("button", { name: "Check saved key" }),
    ).toBeEnabled();
  });

  it("requires an explicit confirmation before deleting the selected native key", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(
      <ProviderAccounts
        accounts={accounts}
        nativeAvailable
        busy={null}
        selectedProviderId="assemblyai"
        onAction={onAction}
      />,
    );

    await user.click(screen.getByRole("button", { name: "Remove key" }));
    const confirmation = screen.getByRole("group", {
      name: "Confirm key removal",
    });
    expect(confirmation).toHaveTextContent("Remove the saved AssemblyAI key?");
    expect(onAction).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Keep key" }));
    expect(
      screen.queryByRole("group", { name: "Confirm key removal" }),
    ).not.toBeInTheDocument();
    expect(onAction).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Remove key" }));
    await user.click(screen.getByRole("button", { name: "Remove saved key" }));
    expect(onAction).toHaveBeenCalledOnce();
    expect(onAction).toHaveBeenCalledWith("assemblyai", "delete");
  });
});
