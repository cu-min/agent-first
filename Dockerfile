FROM node:22-bookworm AS web-builder
WORKDIR /web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.97-bookworm AS server-builder
WORKDIR /app
COPY cargo-mirror.toml /usr/local/cargo/config.toml
COPY server/ ./server/
COPY migrations/ ./migrations/
COPY docs/ ./docs/
WORKDIR /app/server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/server/target \
    cargo build --release && \
    cp target/release/experiencenet /usr/local/bin/experiencenet

FROM debian:bookworm-slim
RUN useradd --system --create-home experiencenet
WORKDIR /app
COPY --from=server-builder /usr/local/bin/experiencenet /usr/local/bin/experiencenet
COPY --from=web-builder /web/dist ./web
ENV BIND_ADDR=0.0.0.0:8080
ENV STATIC_DIR=/app/web
EXPOSE 8080
USER experiencenet
CMD ["experiencenet"]
