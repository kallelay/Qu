/** @type {import('tailwindcss').Config} */
export default {
  // @qu/ui-components is consumed as raw source via a vite.config.ts alias
  // (../qu-ui-components/src), not as a built package -- Tailwind's JIT
  // scanner only generates CSS for classes it finds in these globs, so
  // without the second entry every utility class used inside that package
  // (bg-[#0d0d0d], border-b, etc.) would silently produce no CSS at all.
  content: [
    './index.html',
    './src/**/*.{js,ts,jsx,tsx}',
    '../qu-ui-components/src/**/*.{js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        qu: {
          dark: {
            bg: '#0d0d0d',
            surface: '#1a1a19',
            panel: '#161615',
            border: '#2c2c2a',
            accent: '#3987e5',
            text: '#dfe7f2',
            muted: '#6b7a94',
          },
          light: {
            bg: '#f9f9f7',
            surface: '#fcfcfb',
            panel: '#ffffff',
            border: '#e1e0d9',
            accent: '#2a78d6',
            text: '#52514e',
            muted: '#898781',
          },
        },
      },
    },
  },
  plugins: [],
}
