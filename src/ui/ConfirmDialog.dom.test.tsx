import { describe, expect, it } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { ConfirmHost } from "./ConfirmDialog";
import { confirm } from "./confirm";

describe("confirm()", () => {
  it("renders nothing until asked", () => {
    render(<ConfirmHost />);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("resolves true on confirm and false on cancel", async () => {
    render(<ConfirmHost />);
    let answer: Promise<boolean> | undefined;
    act(() => {
      answer = confirm({ title: "Delete it?", confirmLabel: "Delete", danger: true });
    });
    expect(screen.getByRole("alertdialog")).toBeInTheDocument();
    expect(screen.getByText("Delete it?")).toBeInTheDocument();
    // The safe choice has focus so Enter does not delete by accident.
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await expect(answer).resolves.toBe(true);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    act(() => {
      answer = confirm({ title: "Again?" });
    });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(answer).resolves.toBe(false);
  });

  it("escape cancels", async () => {
    render(<ConfirmHost />);
    let answer: Promise<boolean> | undefined;
    act(() => {
      answer = confirm({ title: "Sure?" });
    });
    fireEvent.keyDown(window, { key: "Escape" });
    await expect(answer).resolves.toBe(false);
  });

  it("a second question cancels the first", async () => {
    render(<ConfirmHost />);
    let first: Promise<boolean> | undefined;
    let second: Promise<boolean> | undefined;
    act(() => {
      first = confirm({ title: "One" });
    });
    act(() => {
      second = confirm({ title: "Two" });
    });
    await expect(first).resolves.toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "OK" }));
    await expect(second).resolves.toBe(true);
  });
});
