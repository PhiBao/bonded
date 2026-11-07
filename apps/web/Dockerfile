# Bonded web — Next.js standalone image (repo-root context).
# Build:  docker build -f apps/web/Dockerfile -t bonded-web .
# Run:    docker run --rm -p 3000:3000 -e API_URL=http://host.docker.internal:8787 bonded-web
FROM node:22-alpine AS build
WORKDIR /app
RUN npm install -g pnpm@11
COPY package.json pnpm-workspace.yaml pnpm-lock.yaml ./
COPY apps/web ./apps/web
RUN pnpm install --frozen-lockfile \
 && API_URL=https://bonded-api.fly.dev pnpm --dir apps/web build

FROM node:22-alpine AS runtime
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /app/apps/web/.next/standalone ./
COPY --from=build /app/apps/web/.next/static ./apps/web/.next/static
COPY --from=build /app/apps/web/public ./apps/web/public
EXPOSE 3000
CMD ["node", "apps/web/server.js"]
