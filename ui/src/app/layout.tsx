import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { ThemeProvider } from "@/lib/theme";
import { AppShellGate } from "@/components/layout/AppShellGate";
import { GlobalErrorBoundary } from "@/components/layout/GlobalErrorBoundary";

// The app's CSS (globals.css) has referenced `var(--font-geist-sans)` since
// the beginning, but nothing ever defined that variable — no next/font
// import, no @font-face, no font files anywhere in the repo. Every page has
// silently been rendering in the browser/OS default fallback font. This is
// what actually loads the intended typeface and injects the CSS variable
// globals.css already expects.
const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"], display: "swap" });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"], display: "swap" });

export const metadata: Metadata = {
  title: "Valori",
  description: "Verifiable memory system for AI agents",
  icons: { icon: "/logo.png" },
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      /* Start in dark — the inline script below immediately corrects to the
         stored preference before first paint, preventing flash of wrong theme. */
      className={`h-full dark antialiased ${geistSans.variable} ${geistMono.variable}`}
      suppressHydrationWarning
    >
      <head>
        {/* FOUC prevention: runs synchronously before CSS is parsed.
            Reads localStorage and applies the correct class before React hydrates. */}
        <script
          dangerouslySetInnerHTML={{
            __html: `(function(){try{var t=localStorage.getItem('valori-theme');var d=t||(window.matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light');document.documentElement.classList.remove('dark','light');document.documentElement.classList.add(d);}catch(e){}})();
              try{fetch('/api/diag-mount?stage=html-script-executed').catch(function(){});}catch(e){}`,
          }}
        />
      </head>
      <body className="flex h-full bg-background text-foreground">
        <ThemeProvider>
          <GlobalErrorBoundary>
            <AppShellGate>{children}</AppShellGate>
          </GlobalErrorBoundary>
        </ThemeProvider>
      </body>
    </html>
  );
}
