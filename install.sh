#!/bin/sh
# Install the latest release of txt to ~/.local/bin
# Usage: curl -fsSL https://raw.githubusercontent.com/ErikHellman/txt/main/install.sh | sh

set -e

REPO="ErikHellman/txt"
BIN_NAME="txt"
INSTALL_DIR="${HOME}/.local/bin"

# ── Detect OS ────────────────────────────────────────────────────────────────
case "$(uname -s)" in
  Darwin) OS="apple-darwin" ;;
  Linux)  OS="unknown-linux-musl" ;;
  *)
    echo "error: unsupported operating system: $(uname -s)" >&2
    exit 1
    ;;
esac

# ── Detect architecture ───────────────────────────────────────────────────────
case "$(uname -m)" in
  x86_64)        ARCH="x86_64" ;;
  arm64|aarch64) ARCH="aarch64" ;;
  *)
    echo "error: unsupported architecture: $(uname -m)" >&2
    exit 1
    ;;
esac

TARGET="${ARCH}-${OS}"

# ── Resolve latest version ────────────────────────────────────────────────────
VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | cut -d'"' -f4)"

if [ -z "${VERSION}" ]; then
  echo "error: could not determine latest version (rate limited?)" >&2
  exit 1
fi

ARCHIVE="${BIN_NAME}-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"

echo "Installing ${BIN_NAME} ${VERSION} (${TARGET}) ..."

# ── Download and extract ──────────────────────────────────────────────────────
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

curl -fsSL "${URL}" -o "${TMP}/${ARCHIVE}"
tar -xzf "${TMP}/${ARCHIVE}" -C "${TMP}"

# ── Install ───────────────────────────────────────────────────────────────────
mkdir -p "${INSTALL_DIR}"
mv "${TMP}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
chmod +x "${INSTALL_DIR}/${BIN_NAME}"

echo "Installed to ${INSTALL_DIR}/${BIN_NAME}"

# ── PATH hint ─────────────────────────────────────────────────────────────────
case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo ""
    echo "Note: ${INSTALL_DIR} is not in your PATH."
    echo "Add the following line to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    echo ""
    echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
    ;;
esac
