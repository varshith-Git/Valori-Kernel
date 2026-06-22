import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { Sidebar } from "@/components/layout/Sidebar";
import { ThemeProvider } from "@/lib/theme";
import { Toaster } from "@/components/ui/Toaster";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Valori Audit Dashboard",
  description: "Real-time BLAKE3 audit trail and vector search",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html
      lang="en"
      /* Start in dark — the inline script below immediately corrects to the
         stored preference before first paint, preventing flash of wrong theme. */
      className={`${geistSans.variable} ${geistMono.variable} h-full dark antialiased`}
    >
      <head>
        {/* FOUC prevention: runs synchronously before CSS is parsed.
            Reads localStorage and applies the correct class before React hydrates. */}
        <script
          dangerouslySetInnerHTML={{
            __html: `(function(){try{var t=localStorage.getItem('valori-theme');var d=t||(window.matchMedia('(prefers-color-scheme:dark)').matches?'dark':'light');document.documentElement.classList.remove('dark','light');document.documentElement.classList.add(d);}catch(e){}})();`,
          }}
        />
      </head>
      <body className="flex h-full bg-background text-foreground">
        <ThemeProvider>
          <Sidebar />
          {/* No top header — full vertical space for content */}
          <main className="flex-1 overflow-auto px-7 py-7">{children}</main>
          <Toaster />
        </ThemeProvider>
      </body>
    </html>
  );
}
