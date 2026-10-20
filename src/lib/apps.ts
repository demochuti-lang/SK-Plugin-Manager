import type { AppId } from '../types';

export interface AppMeta {
  short: string;
  name: string;
  color: string;
}

/** Adobe host app badge metadata (colors per approved mockup). */
export const APP_META: Record<AppId, AppMeta> = {
  ae: { short: 'AE', name: 'After Effects', color: '#8B5CF6' },
  pp: { short: 'PR', name: 'Premiere Pro', color: '#6D5EF0' },
  ps: { short: 'PS', name: 'Photoshop', color: '#31A8FF' },
  ai: { short: 'AI', name: 'Illustrator', color: '#FF9A00' },
};

/** File name from a full path (works for / and \ separators). */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
