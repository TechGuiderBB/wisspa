/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "media",
  theme: {
    extend: {
      colors: {
        accent: "#3b82f6",
      },
      backgroundImage: {
        "wisspa-gradient": "linear-gradient(135deg, #3b82f6 0%, #8b5cf6 100%)",
      },
    },
  },
  plugins: [],
};
