import { useEffect, useRef } from "react";
import { useConfirmStore } from "./confirm";

/** Mount once near the root. Renders whatever `confirm()` is waiting on. */
export function ConfirmHost() {
  const pending = useConfirmStore((state) => state.pending);
  const settle = useConfirmStore((state) => state.settle);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!pending) return;
    const previous = document.activeElement as HTMLElement | null;
    // Focus the safe choice; Enter on a delete should be deliberate.
    cancelRef.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        settle(false);
        return;
      }
      if (event.key === "Tab") {
        // Two buttons: keep focus between them.
        event.preventDefault();
        const next = document.activeElement === cancelRef.current ? confirmRef : cancelRef;
        next.current?.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      previous?.focus?.();
    };
  }, [pending, settle]);

  if (!pending) return null;
  const body = Array.isArray(pending.body) ? pending.body : pending.body ? [pending.body] : [];

  return (
    <div
      role="presentation"
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/55 px-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) settle(false);
      }}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby={body.length ? "confirm-body" : undefined}
        className="w-full max-w-md rounded-xl border border-app-border bg-app-raised p-5 shadow-[0_24px_60px_rgb(0_0_0_/_0.45)]"
      >
        <h2 id="confirm-title" className="text-[16px] font-semibold text-app-text">
          {pending.title}
        </h2>
        {body.length ? (
          <div id="confirm-body" className="mt-2 space-y-2 text-[14px] leading-6 text-app-muted">
            {body.map((line, index) => (
              <p key={index} className="break-words">
                {line}
              </p>
            ))}
          </div>
        ) : null}
        <div className="mt-5 flex justify-end gap-2">
          <button
            ref={cancelRef}
            type="button"
            onClick={() => settle(false)}
            className="rounded-md border border-app-border px-3 py-1.5 text-[13px] font-semibold text-app-text hover:bg-app-hover"
          >
            {pending.cancelLabel ?? "Cancel"}
          </button>
          <button
            ref={confirmRef}
            type="button"
            onClick={() => settle(true)}
            className={`rounded-md px-3 py-1.5 text-[13px] font-semibold ${
              pending.danger ? "bg-app-danger text-white" : "bg-app-play text-app-play-fg"
            }`}
          >
            {pending.confirmLabel ?? "OK"}
          </button>
        </div>
      </div>
    </div>
  );
}
