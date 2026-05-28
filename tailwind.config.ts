import type { Config } from 'tailwindcss'

export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  safelist: [
    'bg-emerald-500',
    'bg-green-500',
    'bg-lime-500',
    'bg-yellow-500',
    'bg-amber-500',
    'bg-orange-500',
    'bg-red-500',
  ],
  theme: {
    extend: {},
  },
  plugins: [],
} satisfies Config
