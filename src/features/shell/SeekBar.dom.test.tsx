import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { SeekBar } from "./SeekBar";

describe("SeekBar", () => {
  it("reflects the position as a range value", () => {
    const { container } = render(
      <SeekBar position={30_000} duration={120_000} onSeek={() => {}} />,
    );
    const input = container.querySelector("input[type=range]") as HTMLInputElement;
    expect(input.max).toBe("120000");
    expect(input.value).toBe("30000");
  });

  it("clamps a position past the end to the duration", () => {
    const { container } = render(
      <SeekBar position={130_000} duration={120_000} onSeek={() => {}} />,
    );
    const input = container.querySelector("input[type=range]") as HTMLInputElement;
    expect(input.value).toBe("120000");
  });

  it("does not divide by zero with no duration", () => {
    const { container } = render(<SeekBar position={0} duration={0} onSeek={() => {}} />);
    const input = container.querySelector("input[type=range]") as HTMLInputElement;
    expect(input.max).toBe("1");
  });

  it("calls onSeek on release, not while dragging", () => {
    const onSeek = vi.fn();
    const { container } = render(<SeekBar position={0} duration={100_000} onSeek={onSeek} />);
    const input = container.querySelector("input[type=range]") as HTMLInputElement;
    fireEvent.pointerDown(input);
    fireEvent.change(input, { target: { value: "45000" } });
    expect(onSeek).not.toHaveBeenCalled();
    fireEvent.pointerUp(input);
    expect(onSeek).toHaveBeenCalledWith(45_000);
  });
});
