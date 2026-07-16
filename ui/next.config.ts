import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  generateBuildId: async () => `build-${Date.now()}`,
  // /audit, /auditor, /auditportal, /audittrail, /snapshot, /snapshots were
  // four+ overlapping audit routes and two identical snapshot routes with no
  // shared mental model. Consolidated into /audit (tabs: Trail/Verify/Export/
  // Third-Party) and /snapshots — these permanently redirect any bookmarked
  // or linked-to orphan URL to its new home.
  async redirects() {
    return [
      { source: "/auditor", destination: "/audit?tab=third-party", permanent: true },
      { source: "/auditportal", destination: "/audit?tab=third-party", permanent: true },
      { source: "/audittrail", destination: "/audit", permanent: true },
      { source: "/snapshot", destination: "/snapshots", permanent: true },
    ];
  },
};

export default nextConfig;
