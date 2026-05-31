import { useState, useEffect, useCallback, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'
import {
  startService,
  stopService,
  isServiceRunning,
  configureRecovery,
} from 'tauri-plugin-background-service'
import {
  CloudUploadOutlined,
  CloudDownloadOutlined,
  GoogleOutlined,
  CheckCircleOutlined,
  ExclamationCircleOutlined,
  LoadingOutlined,
  SyncOutlined,
  ClockCircleOutlined,
  InfoCircleOutlined,
  CloseOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'

type AuthStatus = {
  signed_in: boolean
  email?: string
}

type BackupStatus = {
  signed_in: boolean
  has_backup: boolean
  last_backup: number | null
  backup_size: number | null
  remote_modified?: string
}

type SyncConfig = {
  enabled: boolean
  interval_minutes: number
  wifi_only: boolean
  last_sync_at: number | null
}

type ToastSeverity = 'success' | 'error' | 'info'

interface Toast {
  message: string
  severity: ToastSeverity
  key: number
}

const SYNC_INTERVAL_OPTIONS = [
  { label: 'Every 15 minutes', value: 15 },
  { label: 'Every 30 minutes', value: 30 },
  { label: 'Every hour', value: 60 },
  { label: 'Every 6 hours', value: 360 },
  { label: 'Every 12 hours', value: 720 },
  { label: 'Daily', value: 1440 },
]

export default function GoogleDriveBackup() {
  // Auth state
  const [authStatus, setAuthStatus] = useState<AuthStatus>({
    signed_in: false,
  })
  const [signInStep, setSignInStep] = useState<'idle' | 'signing_in'>('idle')

  // Backup state
  const [backupStatus, setBackupStatus] = useState<BackupStatus | null>(null)
  const [backupAction, setBackupAction] = useState<'idle' | 'backing_up' | 'restoring'>('idle')
  const [showRestoreConfirm, setShowRestoreConfirm] = useState(false)

  // Auto-sync state
  const [syncConfig, setSyncConfig] = useState<SyncConfig>({
    enabled: false,
    interval_minutes: 60,
    wifi_only: true,
    last_sync_at: null,
  })
  const [syncToggled, setSyncToggled] = useState(false)
  const [syncError, setSyncError] = useState<string>('')

  // Toast notification system
  const [toasts, setToasts] = useState<Toast[]>([])
  const toastKeyRef = useRef(0)

  function showToast(message: string, severity: ToastSeverity = 'info') {
    const key = ++toastKeyRef.current
    setToasts((prev) => [...prev, { message, severity, key }])
    if (severity !== 'error') {
      setTimeout(() => dismissToast(key), 5000)
    }
  }

  function dismissToast(key: number) {
    setToasts((prev) => prev.filter((t) => t.key !== key))
  }

  // ─── Load initial state ──────────────────────────────────────────────

  const loadAuthStatus = useCallback(async () => {
    try {
      const status = await invoke<AuthStatus>('google_get_auth_status')
      setAuthStatus(status)
    } catch (e) {
      showToast(`Failed to check auth status: ${e}`, 'error')
    }
  }, [])

  const loadBackupStatus = useCallback(async () => {
    try {
      const status = await invoke<BackupStatus>('google_get_backup_status')
      setBackupStatus(status)
    } catch (e) {
      showToast(`Failed to check backup status: ${e}`, 'error')
    }
  }, [])

  const loadSyncConfig = useCallback(async () => {
    try {
      const config = await invoke<SyncConfig>('google_get_auto_sync')
      setSyncConfig(config)
      if (config.enabled) {
        try {
          const running = await isServiceRunning()
          if (!running) {
            await startService({ serviceLabel: 'LangChef auto-backup' })
          }
          await configureRecovery({
            enabled: true,
            config: { serviceLabel: 'LangChef auto-backup' },
          })
        } catch (svcErr) {
          showToast(`Background service unavailable: ${svcErr}`, 'error')
        }
      }
    } catch (e) {
      showToast(`Failed to load sync settings: ${e}`, 'error')
    }
  }, [])

  useEffect(() => {
    loadAuthStatus()
    loadBackupStatus()
    loadSyncConfig()
  }, [loadAuthStatus, loadBackupStatus, loadSyncConfig])

  // ─── Google Sign In / Out ──────────────────────────────────────────

  async function handleSignIn() {
    setSignInStep('signing_in')
    try {
      const result = await invoke<{ email: string }>('google_sign_in')
      setAuthStatus({ signed_in: true, email: result.email })
      setSignInStep('idle')
      showToast(`Signed in as ${result.email}`, 'success')
      loadBackupStatus()
    } catch (e) {
      showToast(`Sign-in failed: ${e}`, 'error')
      setSignInStep('idle')
    }
  }

  async function handleSignOut() {
    try {
      await invoke('google_sign_out')
      setAuthStatus({ signed_in: false, email: undefined })
      setBackupStatus(null)
      setSyncConfig((prev) => ({ ...prev, enabled: false }))
      try {
        if (await isServiceRunning()) await stopService()
        await configureRecovery({ enabled: false })
      } catch {}
      showToast('Signed out successfully', 'success')
    } catch (e) {
      showToast(`Sign-out failed: ${e}`, 'error')
    }
  }

  // ─── Backup & Restore ──────────────────────────────────────────────

  async function handleBackup() {
    setBackupAction('backing_up')
    try {
      const result = await invoke<{
        success: boolean
        backed_up_at: number
        size: number
      }>('google_backup_db')
      showToast(
        `Backed up successfully (${formatFileSize(result.size)})`,
        'success'
      )
      loadBackupStatus()
      loadSyncConfig()
    } catch (e) {
      showToast(`Backup failed: ${e}`, 'error')
    } finally {
      setBackupAction('idle')
    }
  }

  async function handleRestore() {
    setShowRestoreConfirm(false)
    setBackupAction('restoring')
    try {
      const result = await invoke<{
        success: boolean
        restored_at: number
        size: number
      }>('google_restore_db')
      showToast(
        `Restored successfully (${formatFileSize(result.size)}). Please restart the app.`,
        'success'
      )
      loadBackupStatus()
    } catch (e) {
      showToast(`Restore failed: ${e}`, 'error')
    } finally {
      setBackupAction('idle')
    }
  }

  // ─── Auto-Sync ─────────────────────────────────────────────────────

  async function toggleAutoSync(enabled: boolean) {
    setSyncToggled(true)
    setSyncError('')
    try {
      const config = await invoke<SyncConfig>('google_set_auto_sync', {
        enabled,
        intervalMinutes: syncConfig.interval_minutes,
        wifiOnly: syncConfig.wifi_only,
      })
      setSyncConfig(config)
      try {
        if (enabled) {
          if (!(await isServiceRunning())) {
            await startService({ serviceLabel: 'LangChef auto-backup' })
          }
          await configureRecovery({
            enabled: true,
            config: { serviceLabel: 'LangChef auto-backup' },
          })
        } else {
          if (await isServiceRunning()) await stopService()
          await configureRecovery({ enabled: false })
        }
      } catch (svcErr) {
        const msg = `Background service error: ${svcErr}`
        setSyncError(msg)
        showToast(msg, 'error')
        setSyncToggled(false)
        return
      }
      showToast(enabled ? 'Auto-sync enabled' : 'Auto-sync disabled', 'success')
    } catch (e) {
      const msg = `Failed to save sync settings: ${e}`
      setSyncError(msg)
      showToast(msg, 'error')
    }
    setSyncToggled(false)
  }

  async function changeSyncInterval(intervalMinutes: number) {
    setSyncError('')
    try {
      const config = await invoke<SyncConfig>('google_set_auto_sync', {
        enabled: syncConfig.enabled,
        intervalMinutes,
        wifiOnly: syncConfig.wifi_only,
      })
      setSyncConfig(config)
    } catch (e) {
      const msg = `Failed to update sync interval: ${e}`
      setSyncError(msg)
      showToast(msg, 'error')
    }
  }

  async function triggerSyncNow() {
    setBackupAction('backing_up')
    setSyncError('')
    try {
      const result = await invoke<{
        synced: boolean
        reason: string
        error?: string
      }>('google_trigger_sync')
      if (result.synced) {
        showToast('Auto-sync completed successfully', 'success')
      } else {
        if (result.reason === 'not_due_yet') {
          showToast('Sync not due yet', 'info')
        } else if (result.reason === 'auto_sync_disabled') {
          showToast('Auto-sync is disabled', 'info')
        } else if (result.error) {
          const msg = `Sync error: ${result.error}`
          setSyncError(msg)
          showToast(msg, 'error')
        } else {
          showToast(`Sync skipped: ${result.reason}`, 'info')
        }
      }
      loadBackupStatus()
      loadSyncConfig()
    } catch (e) {
      const msg = `Sync failed: ${e}`
      setSyncError(msg)
      showToast(msg, 'error')
    } finally {
      setBackupAction('idle')
    }
  }

  // ─── Helpers ────────────────────────────────────────────────────────

  function formatTimestamp(ts: number | null): string {
    if (!ts) return 'Never'
    const date = new Date(ts * 1000)
    const now = new Date()
    const diffMs = now.getTime() - date.getTime()
    const diffMin = Math.floor(diffMs / 60000)

    if (diffMin < 1) return 'Just now'
    if (diffMin < 60) return `${diffMin}m ago`
    const diffHr = Math.floor(diffMin / 60)
    if (diffHr < 24) return `${diffHr}h ago`
    const diffDay = Math.floor(diffHr / 24)
    if (diffDay < 7) return `${diffDay}d ago`
    return date.toLocaleDateString()
  }

  function formatFileSize(bytes: number | null): string {
    if (!bytes) return '0 B'
    if (bytes < 1024) return `${bytes} B`
    if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`
    return `${(bytes / 1048576).toFixed(1)} MB`
  }

  // ─── Render ────────────────────────────────────────────────────────

  return (
    <div className="space-y-3">
      <h3 className="text-sm font-medium text-gray-500 uppercase tracking-wide px-1">
        Backup &amp; Sync
      </h3>

      {/* Toast notifications */}
      {toasts.length > 0 && (
        <div className="space-y-2">
          {toasts.map((toast) => (
            <div
              key={toast.key}
              className={`flex items-start gap-2 px-3 py-2.5 rounded-xl text-xs font-medium animate-in fade-in slide-in-from-top-2 ${
                toast.severity === 'success'
                  ? 'bg-green-50 text-green-700 border border-green-100'
                  : toast.severity === 'error'
                  ? 'bg-red-50 text-red-700 border border-red-100'
                  : 'bg-blue-50 text-blue-700 border border-blue-100'
              }`}
            >
              {toast.severity === 'success' ? (
                <CheckCircleOutlined className="mt-0.5 shrink-0" />
              ) : toast.severity === 'error' ? (
                <ExclamationCircleOutlined className="mt-0.5 shrink-0" />
              ) : (
                <InfoCircleOutlined className="mt-0.5 shrink-0" />
              )}
              <span className="flex-1 leading-relaxed">{toast.message}</span>
              <button
                onClick={() => dismissToast(toast.key)}
                className="shrink-0 opacity-50 hover:opacity-100 transition-opacity"
              >
                <CloseOutlined style={{ fontSize: 11 }} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Google Account Section */}
      <div className="bg-white rounded-xl shadow-sm overflow-hidden">
        <div className="px-4 py-3 border-b border-gray-50">
          <label className="flex items-center gap-2 text-sm text-gray-500">
            <GoogleOutlined />
            Google Account
          </label>

          {/* Not signed in: show sign in button */}
          {!authStatus.signed_in && signInStep === 'idle' && (
            <div className="mt-2">
              <button
                onClick={handleSignIn}
                className="flex items-center gap-2 px-4 py-2.5 rounded-lg bg-blue-50 text-blue-600 font-medium text-sm hover:bg-blue-100 active:bg-blue-200 transition-colors"
              >
                <GoogleOutlined />
                Sign in with Google
              </button>
              <p className="text-xs text-gray-400 mt-2">
                Required to back up your data to Google Drive
              </p>
            </div>
          )}

          {/* Signing in: browser opened, waiting for callback */}
          {!authStatus.signed_in && signInStep === 'signing_in' && (
            <div className="flex items-center gap-2 py-2 mt-2">
              <LoadingOutlined spin className="text-blue-500" />
              <span className="text-sm text-gray-500">
                Opening browser for sign-in...
              </span>
            </div>
          )}

          {/* Signed in */}
          {authStatus.signed_in && (
            <div className="mt-2 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <CheckCircleOutlined className="text-green-500" />
                <span className="text-sm">{authStatus.email}</span>
              </div>
              <button
                onClick={handleSignOut}
                className="text-xs text-red-500 hover:text-red-700 font-medium"
              >
                Sign out
              </button>
            </div>
          )}
        </div>

        {/* Backup & Restore Actions */}
        {authStatus.signed_in && (
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-2">
              <CloudUploadOutlined />
              Backup &amp; Restore
            </label>

            {/* Last backup info */}
            {backupStatus && backupStatus.has_backup && (
              <div className="flex items-center justify-between text-xs text-gray-400 mb-3 px-1">
                <span className="flex items-center gap-1">
                  <ClockCircleOutlined />
                  Last backup: {formatTimestamp(backupStatus.last_backup)}
                </span>
                <span>{formatFileSize(backupStatus.backup_size)}</span>
              </div>
            )}

            {/* Action buttons */}
            <div className="flex gap-2">
              <button
                onClick={handleBackup}
                disabled={backupAction === 'backing_up' || backupAction === 'restoring'}
                className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-green-50 text-green-700 font-medium text-sm hover:bg-green-100 active:bg-green-200 transition-colors disabled:opacity-50"
              >
                {backupAction === 'backing_up' ? (
                  <>
                    <LoadingOutlined spin /> Backing up...
                  </>
                ) : (
                  <>
                    <CloudUploadOutlined /> Back Up Now
                  </>
                )}
              </button>
              <button
                onClick={() => setShowRestoreConfirm(true)}
                disabled={
                  backupAction === 'backing_up' ||
                  backupAction === 'restoring' ||
                  !backupStatus?.has_backup
                }
                className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-lg bg-orange-50 text-orange-700 font-medium text-sm hover:bg-orange-100 active:bg-orange-200 transition-colors disabled:opacity-50"
              >
                {backupAction === 'restoring' ? (
                  <>
                    <LoadingOutlined spin /> Restoring...
                  </>
                ) : (
                  <>
                    <CloudDownloadOutlined /> Restore
                  </>
                )}
              </button>
            </div>

            {/* Restore confirmation dialog */}
            {showRestoreConfirm && (
              <div className="fixed inset-0 z-[100] flex items-end sm:items-center justify-center bg-black/40 p-4">
                <div className="bg-white rounded-2xl p-5 w-full max-w-sm shadow-xl animate-in fade-in slide-in-from-bottom-4">
                  <h4 className="text-base font-semibold mb-2">Restore from Backup?</h4>
                  <p className="text-sm text-gray-500 leading-relaxed mb-1">
                    This will replace all local data with your Google Drive backup.
                    Current data will be saved as a safety copy.
                  </p>
                  <p className="text-sm text-orange-600 font-medium mb-4">
                    The app needs to restart after restore.
                  </p>
                  <div className="flex gap-2">
                    <button
                      onClick={() => setShowRestoreConfirm(false)}
                      className="flex-1 py-2.5 rounded-xl bg-gray-100 text-gray-700 font-medium text-sm hover:bg-gray-200 transition-colors"
                    >
                      Cancel
                    </button>
                    <button
                      onClick={handleRestore}
                      className="flex-1 py-2.5 rounded-xl bg-orange-500 text-white font-medium text-sm hover:bg-orange-600 transition-colors"
                    >
                      Yes, Restore
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* Auto-sync Section */}
      {authStatus.signed_in && (
        <div className="bg-white rounded-xl shadow-sm overflow-hidden">
          <div className="px-4 py-3">
            <label className="flex items-center gap-2 text-sm text-gray-500 mb-2">
              <SyncOutlined />
              Auto-Sync Schedule
            </label>

            {/* Toggle row */}
            <div className="flex items-center justify-between py-2">
              <div>
                <p className="text-sm font-medium">Automatic backup</p>
                <p className="text-xs text-gray-400 mt-0.5">
                  Automatically back up your data
                </p>
              </div>
              <button
                onClick={() => toggleAutoSync(!syncConfig.enabled)}
                disabled={syncToggled}
                className={`relative inline-flex h-7 w-12 items-center rounded-full transition-colors ${
                  syncConfig.enabled ? 'bg-blue-500' : 'bg-gray-300'
                } ${syncToggled ? 'opacity-50' : ''}`}
              >
                <span
                  className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
                    syncConfig.enabled ? 'translate-x-6' : 'translate-x-1'
                  }`}
                />
              </button>
            </div>

            {/* Inline sync error */}
            {syncError && (
              <div className="flex items-start gap-2 mt-2 px-2.5 py-2 rounded-lg bg-red-50 text-red-600 text-xs">
                <ExclamationCircleOutlined className="mt-0.5 shrink-0" />
                <span>{syncError}</span>
                <button
                  onClick={() => setSyncError('')}
                  className="shrink-0 ml-auto opacity-50 hover:opacity-100"
                >
                  <CloseOutlined style={{ fontSize: 11 }} />
                </button>
              </div>
            )}

            {/* Interval selector (shown when enabled) */}
            {syncConfig.enabled && (
              <div className="mt-3 pt-3 border-t border-gray-50 space-y-3">
                <div>
                  <p className="text-xs text-gray-400 mb-1.5">Sync interval</p>
                  <select
                    value={syncConfig.interval_minutes}
                    onChange={(e) =>
                      changeSyncInterval(Number(e.target.value))
                    }
                    className="w-full text-sm bg-transparent outline-none cursor-pointer"
                  >
                    {SYNC_INTERVAL_OPTIONS.map((opt) => (
                      <option key={opt.value} value={opt.value}>
                        {opt.label}
                      </option>
                    ))}
                  </select>
                </div>

                {/* Last sync info + manual trigger */}
                {syncConfig.last_sync_at && (
                  <div className="flex items-center justify-between">
                    <span className="text-xs text-gray-400">
                      Last sync: {formatTimestamp(syncConfig.last_sync_at)}
                    </span>
                    <button
                      onClick={triggerSyncNow}
                      disabled={backupAction !== 'idle'}
                      className="flex items-center gap-1 text-xs text-blue-500 hover:text-blue-700 font-medium disabled:opacity-50"
                    >
                      {backupAction === 'backing_up' ? (
                        <>
                          <LoadingOutlined spin /> Syncing...
                        </>
                      ) : (
                        <>
                          <ThunderboltOutlined /> Sync Now
                        </>
                      )}
                    </button>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {/* Info banner */}
      <div className="bg-gray-50 rounded-xl p-4">
        <div className="flex items-start gap-2.5">
          <InfoCircleOutlined className="text-gray-400 mt-0.5 shrink-0" />
          <p className="text-sm text-gray-500 leading-relaxed">
            Your data is encrypted and stored securely in your personal Google
            Drive using the{' '}
            <span className="font-medium">drive.file</span> scope. Only this app
            can access these backups. Like WhatsApp, backups run automatically so
            you never lose your progress when switching devices.
          </p>
        </div>
      </div>
    </div>
  )
}
