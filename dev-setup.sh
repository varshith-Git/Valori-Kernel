#!/usr/bin/env bash
# Valori development environment setup
# Run once after cloning: bash dev-setup.sh
set -euo pipefail

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'
ok()   { echo -e "${GREEN}  ✓${NC} $*"; }
info() { echo -e "${BLUE}  →${NC} $*"; }
warn() { echo -e "${YELLOW}  !${NC} $*"; }
fail() { echo -e "${RED}  ✗ ERROR:${NC} $*"; exit 1; }

echo ""
echo -e "${BLUE}╔══════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   Valori — dev environment setup     ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════╝${NC}"
echo ""

OS="$(uname -s)"
ARCH="$(uname -m)"
info "Detected: $OS / $ARCH"
echo ""

# ── 1. Rust ──────────────────────────────────────────────────────────────────
echo -e "${BLUE}[1/6] Rust${NC}"
if command -v rustc &>/dev/null; then
    RUST_VER="$(rustc --version | awk '{print $2}')"
    ok "Rust $RUST_VER already installed"
else
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
    ok "Rust installed: $(rustc --version)"
fi

# Ensure cargo is on PATH
if ! command -v cargo &>/dev/null; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# Minimum version check (1.80)
RUST_MAJOR=$(rustc --version | awk '{print $2}' | cut -d. -f1)
RUST_MINOR=$(rustc --version | awk '{print $2}' | cut -d. -f2)
if [ "$RUST_MAJOR" -lt 1 ] || { [ "$RUST_MAJOR" -eq 1 ] && [ "$RUST_MINOR" -lt 80 ]; }; then
    info "Updating Rust to stable (need ≥ 1.80)..."
    rustup update stable
fi
echo ""

# ── 2. wasm32 target ─────────────────────────────────────────────────────────
echo -e "${BLUE}[2/6] wasm32 target (for kernel portability checks)${NC}"
if rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
    ok "wasm32-unknown-unknown already installed"
else
    info "Adding wasm32-unknown-unknown target..."
    rustup target add wasm32-unknown-unknown
    ok "wasm32 target added"
fi
echo ""

# ── 3. Python ────────────────────────────────────────────────────────────────
echo -e "${BLUE}[3/6] Python${NC}"
PYTHON_CMD=""
for cmd in python3 python; do
    if command -v "$cmd" &>/dev/null; then
        PY_VER=$($cmd --version 2>&1 | awk '{print $2}')
        PY_MAJOR=$(echo "$PY_VER" | cut -d. -f1)
        PY_MINOR=$(echo "$PY_VER" | cut -d. -f2)
        if [ "$PY_MAJOR" -ge 3 ] && [ "$PY_MINOR" -ge 9 ]; then
            PYTHON_CMD="$cmd"
            ok "Python $PY_VER found ($cmd)"
            break
        fi
    fi
done

if [ -z "$PYTHON_CMD" ]; then
    warn "Python 3.9+ not found."
    if [ "$OS" = "Darwin" ]; then
        if command -v brew &>/dev/null; then
            info "Installing Python via Homebrew..."
            brew install python
            PYTHON_CMD="python3"
            ok "Python installed: $($PYTHON_CMD --version)"
        else
            fail "Please install Python 3.9+ from https://python.org or install Homebrew first."
        fi
    elif [ "$OS" = "Linux" ]; then
        info "Installing Python via apt..."
        sudo apt-get update -qq && sudo apt-get install -y python3 python3-pip
        PYTHON_CMD="python3"
        ok "Python installed: $($PYTHON_CMD --version)"
    else
        fail "Please install Python 3.9+ manually: https://python.org"
    fi
fi
echo ""

# ── 4. Node.js (UI) ──────────────────────────────────────────────────────────
echo -e "${BLUE}[4/6] Node.js (for the UI dashboard)${NC}"
NODE_OK=false
if command -v node &>/dev/null; then
    NODE_VER="$(node --version | sed 's/v//')"
    NODE_MAJOR="$(echo "$NODE_VER" | cut -d. -f1)"
    if [ "$NODE_MAJOR" -ge 18 ]; then
        ok "Node.js v$NODE_VER already installed"
        NODE_OK=true
    else
        warn "Node.js v$NODE_VER is too old (need ≥ 18). Please upgrade: https://nodejs.org"
    fi
else
    warn "Node.js not found. The UI dashboard won't be available."
    if [ "$OS" = "Darwin" ] && command -v brew &>/dev/null; then
        info "Installing Node.js via Homebrew..."
        brew install node
        ok "Node.js installed: $(node --version)"
        NODE_OK=true
    elif [ "$OS" = "Linux" ]; then
        info "Installing Node.js 20 via NodeSource..."
        curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
        sudo apt-get install -y nodejs
        ok "Node.js installed: $(node --version)"
        NODE_OK=true
    else
        warn "Install Node.js 18+ manually from https://nodejs.org then re-run this script."
    fi
fi
echo ""

# ── 5. Build Rust workspace ───────────────────────────────────────────────────
echo -e "${BLUE}[5/6] Build Rust workspace${NC}"
info "Running: cargo build  (this takes a few minutes on first run)"
cargo build
ok "Workspace built successfully"
echo ""

# ── 6. Python SDK ────────────────────────────────────────────────────────────
echo -e "${BLUE}[6/6] Python SDK${NC}"
info "Installing valoricore (remote client) in editable mode..."
$PYTHON_CMD -m pip install -e python/ --quiet
ok "Python SDK installed"
echo ""

# ── UI deps (optional) ────────────────────────────────────────────────────────
if [ "$NODE_OK" = true ] && [ -d "ui" ]; then
    echo -e "${BLUE}[bonus] UI dependencies${NC}"
    info "Running: npm install in ui/"
    (cd ui && npm install --silent)
    ok "UI dependencies installed"
    echo ""
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo -e "${GREEN}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║   Setup complete! Next steps:                    ║${NC}"
echo -e "${GREEN}╚══════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "  Start a node:"
echo -e "  ${YELLOW}VALORI_DIM=128 cargo run -p valori-node${NC}"
echo ""
echo -e "  Run tests:"
echo -e "  ${YELLOW}cargo test -p valori-kernel -p valori-node${NC}"
echo ""
if [ "$NODE_OK" = true ]; then
echo -e "  Start the UI (in a second terminal, after the node is running):"
echo -e "  ${YELLOW}cd ui && npm run dev${NC}  →  http://localhost:3001"
echo ""
fi
echo -e "  Try the Python SDK:"
echo -e "  ${YELLOW}python3 -c \"from valoricore.remote import SyncRemoteClient; print(SyncRemoteClient('http://localhost:3000').health())\"${NC}"
echo ""
echo -e "  Read the full guide: ${BLUE}CONTRIBUTING.md${NC}"
echo ""
