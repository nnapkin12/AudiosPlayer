import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { VirtualList } from "./virtualList";

// jsdom has no ResizeObserver or layout; give the list a fixed viewport.
class FakeResizeObserver {
  observe() {}
  disconnect() {}
}
vi.stubGlobal("ResizeObserver", FakeResizeObserver);
Object.defineProperty(HTMLElement.prototype, "clientHeight", {
  configurable: true,
  get() {
    return 200;
  },
});

const items = Array.from({ length: 1000 }, (_, i) => `row-${i}`);

describe("VirtualList", () => {
  it("renders only the rows near the viewport", () => {
    render(
      <VirtualList
        items={items}
        rowHeight={20}
        overscan={2}
        getKey={(item) => item}
        renderRow={(item) => <span>{item}</span>}
      />,
    );
    // 200px / 20px = 10 visible + 2 * 2 overscan = 14 rows from the top.
    expect(screen.getByText("row-0")).toBeInTheDocument();
    expect(screen.getByText("row-13")).toBeInTheDocument();
    expect(screen.queryByText("row-14")).not.toBeInTheDocument();
    expect(screen.queryByText("row-999")).not.toBeInTheDocument();
  });

  it("sizes the scroll area to the full item count", () => {
    const { container } = render(
      <VirtualList
        items={items}
        rowHeight={20}
        getKey={(item) => item}
        renderRow={(item) => <span>{item}</span>}
      />,
    );
    const spacer = container.firstElementChild?.firstElementChild as HTMLElement;
    expect(spacer.style.height).toBe(`${1000 * 20}px`);
  });

  it("clamps the window when the list shrinks after a deep scroll", () => {
    const { rerender } = render(
      <VirtualList
        items={items}
        rowHeight={20}
        overscan={2}
        getKey={(item) => item}
        renderRow={(item) => <span>{item}</span>}
      />,
    );
    rerender(
      <VirtualList
        items={["a", "b", "c"]}
        rowHeight={20}
        overscan={2}
        getKey={(item) => item}
        renderRow={(item) => <span>{item}</span>}
      />,
    );
    expect(screen.getByText("a")).toBeInTheDocument();
    expect(screen.queryByText("row-13")).not.toBeInTheDocument();
  });

  it("renders nothing for an empty list without throwing", () => {
    const { container } = render(
      <VirtualList<string>
        items={[]}
        rowHeight={20}
        getKey={(item) => item}
        renderRow={(item) => <span>{item}</span>}
      />,
    );
    expect(container.querySelectorAll("span")).toHaveLength(0);
  });
});
