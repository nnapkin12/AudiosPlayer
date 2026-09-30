import { useEffect, useLayoutEffect, useRef, useState } from "react";

export type MenuAction = {
  label: string;
  danger?: boolean;
  disabled?: boolean;
  onClick: () => void;
};

export type MenuEntry =
  | { kind: "action"; action: MenuAction }
  | { kind: "submenu"; label: string; actions: MenuAction[]; pinned?: MenuAction }
  | { kind: "sep" };

const MARGIN = 8;
const GAP = 4;

type FlyoutBox = {
  left: number;
  top: number;
  side: "left" | "right";
};

export function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuEntry[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [openSub, setOpenSub] = useState<string | null>(null);

  useEffect(() => {
    const onPointer = (event: MouseEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) onClose();
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onPointer);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onPointer);
      window.removeEventListener("keydown", onKey);
    };
  }, [onClose]);

  const left = Math.min(x, window.innerWidth - 220);
  const top = Math.min(y, window.innerHeight - 16 - items.length * 32);

  return (
    <div
      ref={ref}
      className="fixed z-50 min-w-[180px] rounded-lg border border-app-border bg-app-raised py-1 shadow-[0_12px_32px_rgb(0_0_0_/_0.35)]"
      style={{ left, top }}
    >
      {items.map((item, index) => {
        if (item.kind === "sep") {
          return <div key={`sep-${index}`} className="my-1 h-px bg-app-line" />;
        }
        if (item.kind === "submenu") {
          return (
            <SubmenuRow
              key={item.label}
              item={item}
              open={openSub === item.label}
              onOpen={() => setOpenSub(item.label)}
              onClose={() => setOpenSub(null)}
              onCloseMenu={onClose}
            />
          );
        }
        return <MenuButton key={item.action.label} action={item.action} onClose={onClose} />;
      })}
    </div>
  );
}

function SubmenuRow({
  item,
  open,
  onOpen,
  onClose,
  onCloseMenu,
}: {
  item: Extract<MenuEntry, { kind: "submenu" }>;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  onCloseMenu: () => void;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const [box, setBox] = useState<FlyoutBox | null>(null);

  useLayoutEffect(() => {
    if (!open) {
      setBox(null);
      return;
    }
    const place = () => {
      const row = rowRef.current;
      const panel = panelRef.current;
      if (!row || !panel) return;
      const next = placeFlyout(row.getBoundingClientRect(), panel.getBoundingClientRect());
      setBox((prev) =>
        prev && prev.left === next.left && prev.top === next.top && prev.side === next.side ? prev : next,
      );
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [open, item.actions.length, item.pinned?.label]);

  return (
    <div className="relative" onMouseEnter={onOpen} onMouseLeave={onClose}>
      <div
        ref={rowRef}
        className="flex w-full items-center justify-between px-3 py-1.5 text-left text-[13px] font-semibold text-app-text hover:bg-app-hover"
      >
        <span>{item.label}</span>
        <span className="text-app-muted">›</span>
      </div>
      {open ? (
        <div
          className="fixed z-10"
          style={{
            left: box?.left ?? 0,
            top: box?.top ?? 0,
            visibility: box ? "visible" : "hidden",
          }}
        >
          <div
            aria-hidden
            className="absolute top-0 h-full w-2"
            style={box?.side === "left" ? { right: -8 } : { left: -8 }}
          />
          <div
            ref={panelRef}
            className="w-max min-w-[160px] max-w-[calc(100vw-16px)] rounded-lg border border-app-border bg-app-raised py-1 shadow-lg"
          >
            {item.pinned ? (
              <>
                <MenuButton action={item.pinned} onClose={onCloseMenu} />
                <div className="my-1 h-px bg-app-line" />
              </>
            ) : null}
            <div className="max-h-[160px] overflow-y-auto overscroll-contain">
              {item.actions.length === 0 ? (
                <p className="px-3 py-1.5 text-[13px] text-app-muted">No playlists yet</p>
              ) : (
                item.actions.map((action, index) => (
                  <MenuButton key={`${action.label}:${index}`} action={action} onClose={onCloseMenu} />
                ))
              )}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}

function placeFlyout(row: DOMRect, panel: DOMRect): FlyoutBox {
  const width = panel.width;
  const height = panel.height;
  const rightLeft = row.right + GAP;
  const leftLeft = row.left - GAP - width;
  const rightFits = rightLeft >= MARGIN && rightLeft + width <= window.innerWidth - MARGIN;
  const leftFits = leftLeft >= MARGIN && leftLeft + width <= window.innerWidth - MARGIN;
  let left = rightLeft;
  let side: FlyoutBox["side"] = "right";
  if (!rightFits && leftFits) {
    left = leftLeft;
    side = "left";
  } else if (!rightFits && !leftFits) {
    const rightRoom = visibleWidth(rightLeft, width);
    const leftRoom = visibleWidth(leftLeft, width);
    if (leftRoom > rightRoom) {
      left = leftLeft;
      side = "left";
    }
  }
  if (left < MARGIN) left = MARGIN;
  const maxLeft = window.innerWidth - MARGIN - width;
  if (maxLeft >= MARGIN && left > maxLeft) left = maxLeft;
  let top = row.top;
  if (top + height > window.innerHeight - MARGIN) {
    top = window.innerHeight - MARGIN - height;
  }
  if (top < MARGIN) top = MARGIN;
  return { left, top, side };
}

function visibleWidth(left: number, width: number): number {
  const start = Math.max(MARGIN, left);
  const end = Math.min(window.innerWidth - MARGIN, left + width);
  return Math.max(0, end - start);
}

function MenuButton({ action, onClose }: { action: MenuAction; onClose: () => void }) {
  return (
    <button
      type="button"
      disabled={action.disabled}
      onClick={() => {
        if (action.disabled) return;
        action.onClick();
        onClose();
      }}
      className={`block w-full px-3 py-1.5 text-left text-[13px] font-semibold disabled:opacity-40 ${
        action.danger ? "text-app-danger hover:bg-app-hover" : "text-app-text hover:bg-app-hover"
      }`}
    >
      {action.label}
    </button>
  );
}
