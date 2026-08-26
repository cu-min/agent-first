FROM node:22-bookworm AS web-builder
WORKDIR /web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.97-bookworm AS server-builder
WORKDIR /app
COPY server/ ./server/
COPY migrations/ ./migrations/
WORKDIR /app/server
RUN cargo build --release

FROM debian:bookworm-slim
RUN useradd --system --create-home agentfirst
WORKDIR /app
COPY --from=server-builder /app/server/target/release/agent-first /usr/local/bin/agent-first
COPY --from=web-builder /web/dist ./web
ENV BIND_ADDR=0.0.0.0:8080
ENV STATIC_DIR=/app/web
EXPOSE 8080
USER agentfirst
CMD ["agent-first"]

