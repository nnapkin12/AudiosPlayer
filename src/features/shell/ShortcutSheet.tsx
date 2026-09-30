import { useEffect, useRef } from "react";
import { useFocusTrap } from "@/ui/useFocusTrap";

const ROWS: Array<[string, string]> = [
  ["Space", "Play / pause"],
  ["← / →", "Seek 5 seconds"],
  ["N / P", "Next / previous"],
  ["F", "Now playing"],
  ["?", "This list"],
  ["Esc", "Close"],
];

export function ShortcutSheet({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, true);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 px-4">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby="shortcut-sheet-title"
        className="w-full max-w-sm rounded-xl border border-app-border bg-app-raised p-5 shadow-[0_24px_60px_rgb(0_0_0_/_0.45)]"
      >
        <div className="flex items-center justify-between">
          <h2 id="shortcut-sheet-title" className="text-[16px] font-semibold">
            Keyboard
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="text-[14px] font-semibold text-app-muted hover:text-app-text"
          >
            Close
          </button>
        </div>
        <ul className="mt-4 space-y-2">
          {ROWS.map(([keys, label]) => (
            <li key={keys} className="flex items-center justify-between gap-4 text-[13px]">
              <span className="font-semibold text-app-muted">{label}</span>
              <kbd className="rounded-md border border-app-border bg-app px-2 py-0.5 font-semibold text-app-text">
                {keys}
              </kbd>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
