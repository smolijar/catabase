export const CONNECTION_ORDER_KEY = 'catabase.connectionOrder';

export function restoreConnectionOrder(saved: string | null): string[] {
  try {
    const order: unknown = JSON.parse(saved ?? 'null');
    return Array.isArray(order) ? order.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return [];
  }
}

export function orderConnections<T extends { id: string }>(records: T[], order: string[]): T[] {
  const remaining = new Map(records.map(record => [record.id, record]));
  const ordered: T[] = [];
  for (const id of order) {
    const record = remaining.get(id);
    if (!record) continue;
    ordered.push(record);
    remaining.delete(id);
  }
  return [...ordered, ...remaining.values()];
}
