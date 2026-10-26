export interface PickerItem {
  id: string;
  label: string;
  title?: string;
  detail: string;
  group: string;
  disabled?: boolean;
  active?: boolean;
  /** Already filtered by the history database (including error text). */
  matched?: boolean;
}

export function pickerMatches(items: PickerItem[], search: string): PickerItem[] {
  const counts = new Map<string, number>();
  const needle = search.trim().toLowerCase();
  return items.filter(item => {
    if (!item.matched && !`${item.label} ${item.detail} ${item.group}`.toLowerCase().includes(needle)) return false;
    const count = counts.get(item.group) ?? 0;
    // A large schema must not hide commands or history below the global cap.
    counts.set(item.group, count + 1);
    return count < (item.group === 'Connections' ? 80 : 20);
  }).slice(0, 80);
}
