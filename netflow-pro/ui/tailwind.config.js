/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  theme: {
    extend: {
      colors: {
        brand: {
          50: "#eef9ff",
          100: "#d9f1ff",
          200: "#bce8ff",
          300: "#8edaff",
          400: "#59c3ff",
          500: "#33a5ff",
          600: "#1b86f5",
          700: "#146de1",
          800: "#1759b6",
          900: "#194c8f",
          950: "#142f57",
        },
      },
    },
  },
  plugins: [],
};
