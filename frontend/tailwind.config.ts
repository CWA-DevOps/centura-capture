import type { Config } from "tailwindcss";

export default {
  content: [
    "./src/pages/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/components/**/*.{js,ts,jsx,tsx,mdx}",
    "./src/app/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        background: "var(--background)",
        foreground: "var(--foreground)",
        // Centura Wealth Advisory brand palette
        primary: "#03639B",   // Sea Blue (brand primary accent)
        secondary: "hsl(210, 40%, 96%)", // gray-50
        accent: "#03639B",    // Sea Blue
        destructive: "hsl(0, 84%, 60%)", // red-500
        centura: {
          DEFAULT: "#03639B",
          blue: "#03639B",     // Sea Blue
          green: "#1B904F",    // Green
          nickel: "#727376",   // Nickel (neutral)
          gainsboro: "#E0DFD7",// Gainsboro (light surface)
          saffron: "#E5C02D",  // Saffron (highlight)
        },
      },
      fontSize: {
        'display': ['32px', { lineHeight: '1.2', fontWeight: '700' }],
        'h1': ['24px', { lineHeight: '1.3', fontWeight: '600' }],
        'h2': ['18px', { lineHeight: '1.4', fontWeight: '500' }],
        'body': ['16px', { lineHeight: '1.6', fontWeight: '400' }],
        'small': ['14px', { lineHeight: '1.5', fontWeight: '400' }],
        'caption': ['12px', { lineHeight: '1.4', fontWeight: '400' }],
      },
    },
  },
  plugins: [
    require('@tailwindcss/typography'),
  ],
} satisfies Config;
