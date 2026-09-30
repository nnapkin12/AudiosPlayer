import type { ReactNode } from "react";
import { Music2, Search, Settings, Tags } from "lucide-react";
import { useAppStore } from "@/store/useAppStore";

export function Sidebar() {
  const tab = useAppStore((state) => state.tab);
  const setTab = useAppStore((state) => state.setTab);

  return (
    <aside className="flex w-16 shrink-0 flex-col items-center border-r border-app-line bg-app-raised py-3">
      <div className="flex flex-col gap-1.5">
        <NavButton active={tab === "player"} label="Audios!" onClick={() => setTab("player")}>
          <Music2 size={20} />
        </NavButton>
        <NavButton active={tab === "search"} label="Search" onClick={() => setTab("search")}>
          <Search size={20} />
        </NavButton>
        <NavButton active={tab === "tags"} label="Tags" onClick={() => setTab("tags")}>
          <Tags size={20} />
        </NavButton>
        <NavButton active={tab === "settings"} label="Settings" onClick={() => setTab("settings")}>
          <Settings size={20} />
        </NavButton>
      </div>
    </aside>
  );
}

function NavButton({
  active,
  label,
  onClick,
  children,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={`flex h-11 w-11 items-center justify-center rounded-lg transition-colors ${
        active
          ? "bg-app-hover text-app-text"
          : "text-app-muted hover:bg-app-hover hover:text-app-subtle"
      }`}
    >
      {children}
    </button>
  );
}
