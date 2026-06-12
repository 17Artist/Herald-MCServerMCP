/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // 与 Herald 桌面端同款 ink 调色板，保持视觉一致。
        ink: {
          950: "#0a0a0c",
          900: "#111114",
          850: "#18181b",
          800: "#1f1f23",
          700: "#27272d",
          600: "#3a3a42",
          500: "#52525b",
          400: "#71717a",
          300: "#a1a1aa",
          200: "#d4d4d8",
          100: "#e4e4e7",
        },
      },
      fontFamily: {
        sans: ['"Inter"', '"PingFang SC"', "ui-sans-serif", "system-ui"],
        mono: ['"JetBrains Mono"', '"Cascadia Code"', "ui-monospace"],
      },
      boxShadow: {
        glow: "0 0 0 1px rgba(167,139,250,0.4), 0 0 24px -4px rgba(167,139,250,0.25)",
      },
    },
  },
  plugins: [],
};
