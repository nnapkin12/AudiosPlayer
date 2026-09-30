import type { ReactNode } from "react";
import { Minus, Square, X } from "lucide-react";
import { windowAction } from "@/lib/api";

export function Titlebar() {
  return (
    <header
      data-tauri-drag-region
      className="drag-region relative z-30 flex h-10 shrink-0 items-center border-b border-app-line bg-app-raised"
    >
      <p
        data-tauri-drag-region
        className="pointer-events-none absolute left-1/2 -translate-x-1/2 text-[14px] font-semibold text-app-subtle"
      >
        Audios!
      </p>
      <div className="no-drag ml-auto flex h-full items-stretch">
        <WinBtn label="Minimize" onClick={() => void windowAction("minimize")}>
          <Minus size={14} strokeWidth={2.2} />
        </WinBtn>
        <WinBtn label="Maximize" onClick={() => void windowAction("toggleMaximize")}>
          <Square size={11} strokeWidth={2.2} />
        </WinBtn>
        <WinBtn label="Close" danger onClick={() => void windowAction("close")}>
          <X size={14} strokeWidth={2.2} />
        </WinBtn>
      </div>
    </header>
  );
}

function WinBtn({
  label,
  danger,
  onClick,
  children,
}: {
  label: string;
  danger?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className={`flex h-full w-11 items-center justify-center text-app-subtle transition-colors ${
        danger ? "hover:bg-app-danger hover:text-white" : "hover:bg-app-hover hover:text-app-text"
      }`}
    >
      {children}
    </button>
  );
}
