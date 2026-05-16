import { useEffect, useRef } from 'react'
import { invoke } from '@tauri-apps/api/core'

export function useActiveTimeHeartbeat(enabled: boolean = true) {
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  useEffect(() => {
    if (!enabled) return

    invoke('db_record_active_time').catch(() => {})

    intervalRef.current = setInterval(() => {
      invoke('db_record_active_time').catch(() => {})
    }, 30_000)

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current)
      }
    }
  }, [enabled])
}