/** A double click may arrive while its first click is still activating the row. */
export function createConnectionInteraction(activate: () => void | Promise<void>, edit: () => void, disabled: () => boolean) {
  let activation: Promise<void> | null = null;
  return {
    use() {
      if (disabled() || activation) return;
      const pending = Promise.resolve(activate());
      activation = pending;
      void pending.catch(() => {}).finally(() => { if (activation === pending) activation = null; });
    },
    async edit() {
      if (activation) { try { await activation; } catch { return; } }
      if (!disabled()) edit();
    }
  };
}
