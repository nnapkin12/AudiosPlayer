import { useEffect, useRef, useState, type ReactNode } from "react";

export function VirtualList<T>({
  items,
  rowHeight,
  overscan = 6,
  className,
  renderRow,
  getKey,
  onPointerLeave,
}: {
  items: T[];
  rowHeight: number;
  overscan?: number;
  className?: string;
  renderRow: (item: T, index: number) => ReactNode;
  getKey: (item: T, index: number) => string;
  onPointerLeave?: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const pending = useRef(0);
  const topRef = useRef(0);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(0);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;
    const frame = () => setHeight(node.clientHeight);
    frame();
    const observer = new ResizeObserver(frame);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => () => cancelAnimationFrame(pending.current), []);

  const visible = Math.ceil((height || 1) / rowHeight) + overscan * 2;
  const start = Math.min(
    Math.max(0, items.length - visible),
    Math.max(0, Math.floor(scrollTop / rowHeight) - overscan),
  );
  const end = Math.min(items.length, start + visible);
  const slice = items.slice(start, end);

  return (
    <div
      ref={ref}
      role="list"
      className={className}
      onPointerLeave={onPointerLeave}
      onScroll={(event) => {
        topRef.current = event.currentTarget.scrollTop;
        if (pending.current) return;
        pending.current = requestAnimationFrame(() => {
          pending.current = 0;
          setScrollTop(topRef.current);
        });
      }}
    >
      <div style={{ height: items.length * rowHeight, position: "relative" }}>
        {slice.map((item, offset) => {
          const index = start + offset;
          return (
            <div
              key={getKey(item, index)}
              role="listitem"
              style={{
                position: "absolute",
                top: index * rowHeight,
                left: 0,
                right: 0,
                height: rowHeight,
              }}
            >
              {renderRow(item, index)}
            </div>
          );
        })}
      </div>
    </div>
  );
}
