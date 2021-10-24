FROM debian:sid-slim AS builder

ARG CLANG_VERSION=13

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
  "clang-$CLANG_VERSION" \
  "lld-$CLANG_VERSION" \
  "llvm-$CLANG_VERSION-dev" \
  ca-certificates \
  curl \
  libssl-dev \
  pkg-config \
  && rm -rf /var/lib/apt/lists/*

ENV \
  RUSTUP_HOME=/usr/local/rustup \
  CARGO_HOME=/usr/local/cargo \
  PATH=/usr/local/cargo/bin:$PATH

WORKDIR /usr/src/app

COPY rust-toolchain ./

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
  sh -s -- -y --default-toolchain none \
  && rustup --version \
  && cargo --version \
  && rustc --version

ENV \
  RUSTFLAGS="-Clinker-plugin-lto=/usr/lib/llvm-$CLANG_VERSION/lib/LLVMgold.so -Clinker=clang-$CLANG_VERSION -Clink-arg=-fuse-ld=lld-$CLANG_VERSION -Clink-arg=-v" \
  CC="clang-$CLANG_VERSION -flto" \
  CXX="clang++-$CLANG_VERSION -flto"

COPY Cargo.toml Cargo.lock ./
# Create a fake `src`, build deps and remove the fake `src`.
# See https://github.com/rust-lang/cargo/issues/2644.
RUN mkdir src \
  && touch src/lib.rs \
  && cargo build --locked --release --verbose \
  && rm -rf src

COPY src src
RUN ls -la \
  && cargo build --frozen --release \
  && cp target/release/musicbot2 /usr/local/bin \
  && ldd /usr/local/bin/musicbot2

FROM debian:sid-slim

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
  ca-certificates \
  libssl-dev \
  libcrypto++-dev \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/musicbot2 /usr/local/bin/musicbot2

RUN ["ldd", "/usr/local/bin/musicbot2"]

CMD ["musicbot2"]
