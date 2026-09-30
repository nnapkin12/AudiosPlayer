import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { ErrorBoundary } from "./ErrorBoundary";

function Bomb({ explode }: { explode: boolean }) {
  if (explode) throw new Error("kaboom");
  return <p>fine</p>;
}

describe("ErrorBoundary", () => {
  it("shows the fallback with the message and keeps siblings alive", () => {
    const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <div>
        <p>titlebar</p>
        <ErrorBoundary name="the library">
          <Bomb explode />
        </ErrorBoundary>
      </div>,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Something went wrong in the library.");
    expect(screen.getByText("kaboom")).toBeInTheDocument();
    expect(screen.getByText("titlebar")).toBeInTheDocument();
    quiet.mockRestore();
  });

  it("try again re-renders the children", () => {
    const quiet = vi.spyOn(console, "error").mockImplementation(() => {});
    let explode = true;
    const { rerender } = render(
      <ErrorBoundary name="Tags">
        <Bomb explode={explode} />
      </ErrorBoundary>,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    explode = false;
    rerender(
      <ErrorBoundary name="Tags">
        <Bomb explode={explode} />
      </ErrorBoundary>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByText("fine")).toBeInTheDocument();
    quiet.mockRestore();
  });
});
