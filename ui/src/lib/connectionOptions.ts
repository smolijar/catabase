import type { ConnectionOptions } from './types';

export function connectionOptions(options?: Partial<ConnectionOptions>): ConnectionOptions {
  return { connectTimeoutSeconds: 8, queryTimeoutSeconds: 0, lockTimeoutSeconds: 0, readOnly: false, ...options };
}

export function connectionTestTimeout(options?: ConnectionOptions): number {
  return (connectionOptions(options).connectTimeoutSeconds + 4) * 1000;
}
