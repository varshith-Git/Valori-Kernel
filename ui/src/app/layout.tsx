import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";
import { Sidebar } from "@/components/layout/Sidebar";
import { ConnectionBadge } from "@/components/layout/ConnectionBadge";

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
      className={`${geistSans.variable} ${geistMono.variable} h-full dark antialiased`}
    >
      <body className="flex h-full bg-zinc-950 text-zinc-100">
        <Sidebar />
        <div className="flex flex-1 flex-col overflow-hidden">
          {/* Top bar */}
          <header className="flex h-12 items-center justify-between border-b border-zinc-800 px-6">
            <span className="text-xs text-zinc-500 font-mono">
              deterministic · tamper-evident · Q16.16
            </span>
            <ConnectionBadge />
          </header>
          <main className="flex-1 overflow-auto px-6 py-6">{children}</main>
        </div>
      </body>
    </html>
  );
}
