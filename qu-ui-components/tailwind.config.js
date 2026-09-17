/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{ts,tsx}'],
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
      backdropBlur: {
        xs: '2px',
      },
      animation: {
        'fade-in': 'fadeIn 0.2s ease-out',
        'slide-in': 'slideIn 0.2s ease-out',
        'scale-in': 'scaleIn 0.15s ease-out',
      },
      keyframes: {
        fadeIn: {
          '0%': { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideIn: {
          '0%': { transform: 'translateX(-20px)', opacity: '0' },
          '100%': { transform: 'translateX(0)', opacity: '1' },
        },
        scaleIn: {
          '0%': { transform: 'scale(0.95)', opacity: '0' },
          '100%': { transform: 'scale(1)', opacity: '1' },
        },
      },
    },
  },
  plugins: [],
}
