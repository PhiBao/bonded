import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  reactStrictMode: true,
  async rewrites() {
    const apiUrl =
      process.env.API_URL ??
      (process.env.VERCEL ? "https://bonded-api.fly.dev" : "http://127.0.0.1:8787");
    return [
      {
        source: "/api/:path*",
        destination: `${apiUrl}/api/:path*`,
      },
    ];
  },
};

export default nextConfig;
