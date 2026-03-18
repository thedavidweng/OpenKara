# OpenKara Linux build environment
#
# Provides a reproducible Debian-based image with all dependencies required
# to build OpenKara on Linux: Node.js, pnpm, Rust, and the system libraries
# needed by Tauri and the audio stack.

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

# System libraries required by Tauri and the audio stack
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    file \
    patchelf \
    libasound2-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    libssl-dev \
    libwebkit2gtk-4.1-dev \
    libxdo-dev \
    wget \
    && rm -rf /var/lib/apt/lists/*

# Node.js 20
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# pnpm (pinned to the version used in the project)
RUN npm install -g pnpm@10.12.1

# Rust (stable)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- --default-toolchain stable --profile minimal -y

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app
