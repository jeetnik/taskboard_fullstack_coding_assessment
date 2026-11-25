import type { NextConfig } from "next";

const apiOrigin = (process.env.API_ORIGIN ?? "http://127.0.0.1:4000").replace(/\/$/, "");

const config: NextConfig = {
  poweredByHeader: false,
  async rewrites() {
    return [{ source: "/api/:path*", destination: `${apiOrigin}/:path*` }];
  },
};

export default config;
