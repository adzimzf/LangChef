import { useEffect, useState } from 'react'

/** Shared motion helpers. CSS animations live in index.css (`anim-*`); these are the ones that need script. */

export const reducedMotion = () => typeof window !== 'undefined' && !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches

/** Number that counts up from 0 when it first shows. */
export function CountUp({ value }: { value: number }) {
  const [shown, setShown] = useState(() => (reducedMotion() ? value : 0))
  useEffect(() => {
    if (reducedMotion()) { setShown(value); return }
    let frame = 0
    const start = performance.now()
    const step = (now: number) => {
      const k = Math.min(1, (now - start) / 900)
      setShown(Math.round(value * (1 - Math.pow(1 - k, 3))))
      if (k < 1) frame = requestAnimationFrame(step)
    }
    frame = requestAnimationFrame(step)
    return () => cancelAnimationFrame(frame)
  }, [value])
  return <>{shown}</>
}

/** Paper confetti for a good result. */
export function burstConfetti() {
  if (reducedMotion()) return
  const canvas = document.createElement('canvas')
  canvas.className = 'anim-confetti'
  document.body.appendChild(canvas)
  const dpr = window.devicePixelRatio || 1
  const w = window.innerWidth
  const h = window.innerHeight
  canvas.width = w * dpr
  canvas.height = h * dpr
  const g = canvas.getContext('2d')
  if (!g) { canvas.remove(); return }
  g.scale(dpr, dpr)
  const colors = ['#3b82f6', '#22c55e', '#f59e0b', '#e11d48', '#6366f1']
  const bits = Array.from({ length: 110 }, () => ({
    x: w / 2 + (Math.random() - 0.5) * 80, y: h * 0.3,
    vx: (Math.random() - 0.5) * 9, vy: -Math.random() * 9 - 3,
    r: Math.random() * 6.28, vr: (Math.random() - 0.5) * 0.3,
    s: 5 + Math.random() * 5, c: colors[Math.floor(Math.random() * colors.length)],
  }))
  const t0 = performance.now()
  const frame = (now: number) => {
    g.clearRect(0, 0, w, h)
    for (const b of bits) {
      b.vy += 0.25; b.vx *= 0.99; b.x += b.vx; b.y += b.vy; b.r += b.vr
      g.save(); g.translate(b.x, b.y); g.rotate(b.r); g.fillStyle = b.c; g.fillRect(-b.s / 2, -b.s / 4, b.s, b.s / 2); g.restore()
    }
    if (now - t0 < 2600) requestAnimationFrame(frame)
    else canvas.remove()
  }
  requestAnimationFrame(frame)
}
