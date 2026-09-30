import { openExternal } from "@/lib/api";
import { GITHUB_URL } from "@/lib/links";
import { useAppStore } from "@/store/useAppStore";

export function LibraryHome({
  onAddFolder,
  onOpenFile,
}: {
  onAddFolder: () => void;
  onOpenFile: () => void;
}) {
  const empty = useAppStore((state) => state.libraryRoots.length === 0);

  if (empty) {
    return (
      <div className="flex min-h-0 flex-1 flex-col items-center justify-center px-6 pb-10">
        <img
          src="/audios.png"
          alt=""
          className="w-full max-w-[200px] rounded-2xl bg-white object-contain shadow-[0_18px_40px_rgb(0_0_0_/_0.35)]"
        />
        <h1 className="mt-6 text-[clamp(1.6rem,5vw,2.4rem)] font-semibold tracking-tight text-app-text">
          Add your music
        </h1>
        <p className="mt-3 max-w-md text-center text-[15px] font-medium leading-6 text-app-muted">
          Drop a folder on this window, or pick one. Artwork and albums come from the tags in those
          files.
        </p>
        <div className="mt-6 flex flex-wrap justify-center gap-2">
          <button
            type="button"
            onClick={onAddFolder}
            className="rounded-md bg-app-play px-3 py-1.5 text-[13px] font-semibold text-app-play-fg"
          >
            Add folder
          </button>
          <button
            type="button"
            onClick={onOpenFile}
            className="rounded-md px-3 py-1.5 text-[13px] font-semibold text-app-subtle hover:bg-app-hover"
          >
            Open file
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center px-8 pb-16">
      <img
        src="/audios.png"
        alt="Audios!"
        className="w-full max-w-[360px] rounded-2xl bg-white object-contain shadow-[0_18px_40px_rgb(0_0_0_/_0.35)]"
      />
      <h1 className="mt-8 text-[42px] font-semibold tracking-tight text-app-text">Audios!</h1>
      <p className="mt-3 max-w-lg text-center text-[15px] font-medium leading-6 text-app-muted">
        Audios! takes a folder of audio files and plays them as a library. Artwork, artists, and
        albums come from the tags in those files.
      </p>
      <button
        type="button"
        onClick={() => void openExternal(GITHUB_URL)}
        className="mt-4 text-[14px] font-semibold text-app-accent underline-offset-2 hover:underline"
      >
        Star this on GitHub
      </button>
    </div>
  );
}
