#!/usr/bin/env bash
# One-time setup for lnp2 on the remote (lumeneer.local)
# Run via: just setup

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

ok()   { echo -e "  ${GREEN}✓${NC} $1"; }
warn() { echo -e "  ${YELLOW}!${NC} $1"; }
fail() { echo -e "  ${RED}✗${NC} $1"; }

echo "── lnp2 remote setup ──"
echo ""

# Source cargo env if available
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

# ── Directory structure ───────────────────────────────────────────────

echo "Directories:"
mkdir -p ~/lnp2/builds ~/lnp2/production/config ~/lnp2/production/web
ok "~/lnp2/{builds,production} created"

# ── User groups ───────────────────────────────────────────────────────

echo ""
echo "Groups:"
needs_relogin=false

if groups | grep -q dialout; then
    ok "dialout (serial port access)"
else
    warn "dialout — run: sudo usermod -aG dialout $USER"
    needs_relogin=true
fi

if groups | grep -q video; then
    ok "video (camera access)"
else
    warn "video — run: sudo usermod -aG video $USER"
    needs_relogin=true
fi

if $needs_relogin; then
    warn "Log out and back in for group changes to take effect"
fi

# ── Toolchain ─────────────────────────────────────────────────────────

echo ""
echo "Toolchain:"

if command -v rustc &>/dev/null; then
    ok "rustc $(rustc --version | awk '{print $2}')"
else
    fail "rustc not found — install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

if command -v cargo &>/dev/null; then
    ok "cargo $(cargo --version | awk '{print $2}')"
else
    fail "cargo not found"
fi

if command -v node &>/dev/null; then
    ok "node $(node --version)"
else
    fail "node not found — install: https://nodejs.org/"
fi

if command -v npm &>/dev/null; then
    ok "npm $(npm --version)"
else
    fail "npm not found"
fi

# ── System dependencies ──────────────────────────────────────────────

echo ""
echo "System libraries:"

DEPS=(libopencv-dev clang libclang-dev pkg-config libudev-dev v4l-utils)
missing=()

for dep in "${DEPS[@]}"; do
    if dpkg -s "$dep" &>/dev/null; then
        ok "$dep"
    else
        fail "$dep"
        missing+=("$dep")
    fi
done

if [ ${#missing[@]} -gt 0 ]; then
    echo ""
    warn "Install missing deps:"
    echo "  sudo apt install -y ${missing[*]}"
fi

# ── ONNX Runtime ─────────────────────────────────────────────────────

echo ""
echo "ONNX Runtime:"
if [ -f /usr/lib/libonnxruntime.so ] || ldconfig -p 2>/dev/null | grep -q libonnxruntime; then
    ok "libonnxruntime found"
else
    warn "libonnxruntime not found in system paths"
    if [ -n "${ORT_DYLIB_PATH:-}" ]; then
        ok "ORT_DYLIB_PATH set: $ORT_DYLIB_PATH"
    else
        warn "Set ORT_DYLIB_PATH or install onnxruntime to system lib path"
    fi
fi

# ── Sudoers hint ─────────────────────────────────────────────────────

echo ""
echo "── Optional: passwordless systemctl ──"
echo "  Add to /etc/sudoers.d/lnp2:"
echo "  $USER ALL=(ALL) NOPASSWD: /usr/bin/systemctl start lnp2, /usr/bin/systemctl stop lnp2, /usr/bin/systemctl restart lnp2, /usr/bin/systemctl daemon-reload, /usr/bin/systemctl enable lnp2, /usr/bin/systemctl status lnp2"
echo ""
echo "  sudo visudo -f /etc/sudoers.d/lnp2"
echo ""
echo "Done."
