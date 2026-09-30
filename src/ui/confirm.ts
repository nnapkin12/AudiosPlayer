import { create } from "zustand";

export interface ConfirmOptions {
  title: string;
  /** One or more paragraphs. */
  body?: string | string[];
  confirmLabel?: string;
  cancelLabel?: string;
  /** Red confirm button for deletes and other one-way actions. */
  danger?: boolean;
}

interface Pending extends ConfirmOptions {
  resolve: (ok: boolean) => void;
}

interface ConfirmState {
  pending: Pending | null;
  ask: (options: ConfirmOptions) => Promise<boolean>;
  settle: (ok: boolean) => void;
}

export const useConfirmStore = create<ConfirmState>((set, get) => ({
  pending: null,
  ask: (options) =>
    new Promise<boolean>((resolve) => {
      // A second question while one is open cancels the first.
      get().pending?.resolve(false);
      set({ pending: { ...options, resolve } });
    }),
  settle: (ok) => {
    const pending = get().pending;
    set({ pending: null });
    pending?.resolve(ok);
  },
}));

/**
 * Ask before a one-way action. Resolves `true` only when the user confirms.
 *
 * ```ts
 * if (!(await confirm({ title: "Delete playlist?", danger: true }))) return;
 * ```
 */
export function confirm(options: ConfirmOptions): Promise<boolean> {
  return useConfirmStore.getState().ask(options);
}
