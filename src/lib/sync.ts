import { t, formatNumber } from './i18n'

/** Result of one sync with Google Drive (`sync_now` in `google_drive.rs`). */
export type SyncResult = { sent: number; received: number; snapshot: 'full' | 'delta' | 'none'; size: number }

/** Payload of the `auto-sync` event Rust sends around each automatic sync. */
export type AutoSyncEvent =
  | { state: 'syncing' }
  | ({ state: 'done' } & SyncResult)
  | { state: 'error'; error?: string }

export function syncResultText(result: Pick<SyncResult, 'sent' | 'received'>): string {
  if (!result.sent && !result.received) return t('backup.upToDate')
  return t('backup.synced', { sent: formatNumber(result.sent), received: formatNumber(result.received) })
}
