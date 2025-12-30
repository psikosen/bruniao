/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './src/pages/**/*.{js,ts,jsx,tsx,mdx}',
    './src/components/**/*.{js,ts,jsx,tsx,mdx}',
    './src/app/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      colors: {
        'poly-green': '#00D395',
        'poly-red': '#FF6B6B',
        'poly-blue': '#4A90D9',
        'poly-dark': '#1a1a2e',
        'poly-darker': '#0f0f1a',
      },
    },
  },
  plugins: [],
}
