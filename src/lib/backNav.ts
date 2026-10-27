/**
 * Android back (gesture or button), the way Android users expect it:
 *   open sheet or dialog → closes it; a step inside a page → the step before; a tab → Home; Home → leaves the app.
 *
 * Tauri's Android shell goes back in the webview history and leaves the app once there is nothing to go
 * back to, so everything here is plain browser history, also on desktop:
 * - Tabs keep the history at [Home, tab] (`openTab`), so back from any tab is Home.
 * - While something on screen handles back (`useBackHandler`), a "guard" entry with the same URL sits on top
 *   of the history. Going back leaves the guard; the newest handler runs and the guard comes back if
 *   anything still handles back. When nothing does, the guard is taken off again.
 *
 * Guards copy React Router's history state, so the router sees the same location and doesn't re-render a page.
 */
import { useEffect, useRef } from 'react'
import type { NavigateFunction } from 'react-router-dom'

const GUARD = 'langchefBackGuard'

type Handler = { onBack: () => void }

/** What handles back now, newest last. Mirrors what is open on screen, so it is not app data. */
const handlers: Handler[] = []
/** Whether our guard entry is the current history entry. */
let guarded = false
/** A `history.back()` we started to take the guard off: its popstate isn't the learner pressing back. */
let ownPops = 0

const onGuard = () => !!(window.history.state as Record<string, unknown> | null)?.[GUARD]

function sync() {
  if (guarded && !onGuard()) guarded = false // a navigation went on top of the guard
  if (handlers.length > 0 && !guarded) {
    window.history.pushState({ ...(window.history.state ?? {}), [GUARD]: true }, '')
    guarded = true
  } else if (handlers.length === 0 && guarded) {
    guarded = false
    ownPops += 1
    window.history.back()
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('popstate', () => {
    if (ownPops > 0) {
      ownPops -= 1
      guarded = onGuard()
      sync()
      return
    }
    if (guarded && !onGuard()) {
      // Back left our guard: the newest handler takes it.
      guarded = false
      handlers[handlers.length - 1]?.onBack()
      sync()
      return
    }
    if (onGuard()) {
      // Landed on a guard: ours if something still handles back, otherwise one left behind by a
      // navigation, which back skips.
      if (handlers.length > 0) guarded = true
      else window.history.back()
    }
  })
}

/** While `active`, back calls `onBack` instead of leaving the page. The newest active handler goes first. */
export function useBackHandler(active: boolean, onBack: () => void) {
  const latest = useRef(onBack)
  latest.current = onBack
  useEffect(() => {
    if (!active) return
    const handler: Handler = { onBack: () => latest.current() }
    handlers.push(handler)
    sync()
    return () => {
      const i = handlers.indexOf(handler)
      if (i >= 0) handlers.splice(i, 1)
      sync()
    }
  }, [active])
}

/** How many history entries back the first one (Home) is. React Router numbers its entries in `idx`. */
function stepsToHome(): number {
  const state = window.history.state as { idx?: unknown } | null
  const idx = typeof state?.idx === 'number' ? state.idx : 0
  return idx + (onGuard() ? 1 : 0)
}

/** Back to Home, the first history entry, so that back from Home leaves the app. */
export function goHome(navigate: NavigateFunction) {
  const steps = stepsToHome()
  if (steps > 0) navigate(-steps)
  else if (window.location.pathname !== '/') navigate('/', { replace: true })
}

/**
 * Opens a tab so the history is [Home, tab]. From Home it is pushed; from anywhere else the history first goes
 * back to Home, and `afterHome` (called by Layout once Home is shown) pushes the tab.
 */
export function openTab(navigate: NavigateFunction, path: string, pending: { current: string | null }) {
  if (path === '/') {
    pending.current = null
    goHome(navigate)
  } else if (stepsToHome() === 0 && window.location.pathname === '/') {
    navigate(path)
  } else {
    pending.current = path
    goHome(navigate)
  }
}

/** Finishes `openTab` once the history is back at Home. Returns true while a tab is still on its way. */
export function afterHome(navigate: NavigateFunction, pathname: string, pending: { current: string | null }): boolean {
  if (!pending.current) return false
  if (pathname !== '/') return true
  const path = pending.current
  pending.current = null
  navigate(path)
  return false
}
