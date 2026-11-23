import { writable } from 'svelte/store';

const STORAGE_KEY = 'catabase.activity';
const MAX_EVENTS = 200;

export type ActivityCategory = 'clipboard' | 'connection' | 'schema';
export type ActivityStatus = 'running' | 'completed' | 'failed' | 'interrupted';

export interface ActivityEvent {
  id: string;
  category: ActivityCategory;
  title: string;
  subject?: string;
  status: ActivityStatus;
  startedAt: string;
  finishedAt?: string;
  elapsedMs?: number;
  detail?: string;
}

type ActivityLog = { events: readonly ActivityEvent[]; clearedAt: number | null };

function restoreLog(): ActivityLog {
  try {
    const saved: ActivityEvent[] | ActivityLog | null = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? 'null');
    const savedEvents = Array.isArray(saved) ? saved : saved?.events;
    const clearedAt = Array.isArray(saved) ? null : saved?.clearedAt ?? null;
    const events = (Array.isArray(savedEvents) ? savedEvents : []).slice(-MAX_EVENTS)
      .filter(event => event && typeof event.id === 'string' && typeof event.title === 'string'
        && Number.isFinite(Date.parse(event.startedAt))
        && ['clipboard', 'connection', 'schema'].includes(event.category)
        && ['running', 'completed', 'failed', 'interrupted'].includes(event.status))
      .map((event): ActivityEvent => event.status === 'running' ? { ...event, status: 'interrupted' } : event);
    return { events, clearedAt: Number.isSafeInteger(clearedAt) ? clearedAt : null };
  } catch {
    return { events: [], clearedAt: null };
  }
}

let log = restoreLog();
const store = writable(log);
export const activityLog = { subscribe: store.subscribe };

function publish(events: ActivityEvent[], clearedAt = log.clearedAt) {
  log = { events: events.slice(-MAX_EVENTS), clearedAt };
  store.set(log);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(log));
  } catch {
    // Storage may be unavailable. The current window still has the activity.
  }
}

export function clearActivityLog() {
  // A fresh History response can still contain SQL from before the clear.
  publish([], Date.now());
}

export function startActivity(category: ActivityCategory, title: string, subject?: string) {
  const event: ActivityEvent = {
    id: crypto.randomUUID(), category, title, subject: subject?.slice(0, 200),
    status: 'running', startedAt: new Date().toISOString()
  };
  const started = performance.now();
  publish([...log.events, event]);
  return (status: 'completed' | 'failed', detail?: string) => {
    const finishedAt = new Date().toISOString();
    const elapsedMs = Math.round(performance.now() - started);
    publish(log.events.map(current => current.id === event.id ? {
      ...current, status, finishedAt, elapsedMs, detail: detail?.slice(0, 400)
    } : current));
  };
}
