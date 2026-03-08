# lnp2 remote dev workflow
# Run `just` with no args to see all available recipes

set dotenv-load := false

# Remote host
remote_host := "bleu@lumeneer.local"

# Branch name (slashes replaced with dashes)
branch := `git rev-parse --abbrev-ref HEAD | tr '/' '-'`

# Remote paths
remote_base := "/home/bleu/lnp2"
remote_build := remote_base / "builds" / branch
remote_prod := remote_base / "production"

# Source cargo env + set ORT path for non-interactive SSH
remote_env := 'source \$HOME/.cargo/env 2>/dev/null; export ORT_DYLIB_PATH=\$HOME/onnxruntime-linux-x64-1.24.2/lib/libonnxruntime.so;'

# Rsync excludes
rsync_excludes := "--exclude target/ --exclude node_modules/ --exclude web/dist/ --exclude .git/"

# ── Sync ──────────────────────────────────────────────────────────────

# Sync source to remote build dir for current branch
sync:
    @echo "Syncing to {{remote_host}}:{{remote_build}}/"
    ssh {{remote_host}} "mkdir -p {{remote_build}}"
    rsync -az --delete {{rsync_excludes}} ./ {{remote_host}}:{{remote_build}}/
    @echo "Done."

# ── Build ─────────────────────────────────────────────────────────────

# Sync + cargo build --release on remote
build: sync
    @echo "Building Rust on {{remote_host}} ({{branch}})..."
    ssh -tt {{remote_host}} "{{remote_env}} cd {{remote_build}} && cargo build --release"

# Sync + npm install + vite build on remote
build-web: sync
    @echo "Building web on {{remote_host}} ({{branch}})..."
    ssh -tt {{remote_host}} "cd {{remote_build}}/web && npm install --silent && npx vite build"

# ── Test ──────────────────────────────────────────────────────────────

# Sync + cargo test on remote
test: sync
    @echo "Testing on {{remote_host}} ({{branch}})..."
    ssh -tt {{remote_host}} "{{remote_env}} cd {{remote_build}} && cargo test"

# ── Run ───────────────────────────────────────────────────────────────

# Build + run server in foreground (stops systemd service first)
run: build build-web
    @echo "Stopping lnp2 service (if running)..."
    -ssh -tt {{remote_host}} "sudo systemctl stop lnp2 2>/dev/null"
    @echo "Starting server in foreground (Ctrl+C to stop)..."
    ssh -tt {{remote_host}} "{{remote_env}} cd {{remote_build}} && ./target/release/lnp2 serve --config config"

# ── Deploy ────────────────────────────────────────────────────────────

# Build all + copy to production dir + restart systemd service
deploy: build build-web
    @echo "Deploying to production..."
    ssh {{remote_host}} "mkdir -p {{remote_prod}}/config {{remote_prod}}/web"
    ssh {{remote_host}} "cp {{remote_build}}/target/release/lnp2 {{remote_prod}}/lnp2"
    ssh {{remote_host}} "rsync -a {{remote_build}}/config/ {{remote_prod}}/config/"
    ssh {{remote_host}} "rsync -a --delete {{remote_build}}/web/dist/ {{remote_prod}}/web/dist/"
    ssh -tt {{remote_host}} "sudo systemctl restart lnp2"
    @echo "Deployed and restarted."
    @just status

# Deploy binary only (skip web build)
deploy-rust: build
    @echo "Deploying binary to production..."
    ssh {{remote_host}} "cp {{remote_build}}/target/release/lnp2 {{remote_prod}}/lnp2"
    ssh {{remote_host}} "rsync -a {{remote_build}}/config/ {{remote_prod}}/config/"
    ssh -tt {{remote_host}} "sudo systemctl restart lnp2"
    @echo "Deployed and restarted."
    @just status

# ── Service Management ───────────────────────────────────────────────

# View service logs (follows by default)
logs n="50":
    ssh -tt {{remote_host}} "journalctl -u lnp2 -n {{n}} -f"

# Show systemd service status
status:
    @ssh {{remote_host}} "systemctl status lnp2 2>&1" || true

# Install/update systemd service unit file
service-install: sync
    @echo "Installing systemd service..."
    ssh -tt {{remote_host}} "sudo cp {{remote_build}}/remote/lnp2.service /etc/systemd/system/lnp2.service && sudo systemctl daemon-reload && sudo systemctl enable lnp2"
    @echo "Service installed and enabled."

# ── Utilities ─────────────────────────────────────────────────────────

# SSH into remote build dir for current branch
ssh:
    @echo "Connecting to {{remote_host}}:{{remote_build}}/"
    ssh -tt {{remote_host}} "cd {{remote_build}} && exec \$SHELL -l"

# List remote build dirs with disk usage
builds:
    @ssh {{remote_host}} "echo '── lnp2 build dirs ──' && du -sh {{remote_base}}/builds/*/ 2>/dev/null || echo '(none)'"

# Remove a remote build dir (defaults to current branch)
clean target=branch:
    @echo "Removing remote build dir: {{target}}"
    ssh {{remote_host}} "rm -rf {{remote_base}}/builds/{{target}}"
    @echo "Removed."

# Auto-sync on file save (requires: brew install fswatch)
watch:
    @echo "Watching for changes... (Ctrl+C to stop)"
    fswatch -o -e '.git' -e 'target' -e 'node_modules' -e 'web/dist' . | while read -r _; do just sync; done

# ── Setup & Diagnostics ──────────────────────────────────────────────

# Run one-time remote setup script
setup: sync
    @echo "Running remote setup..."
    ssh -tt {{remote_host}} "bash {{remote_build}}/remote/setup.sh"

# Verify remote environment (rust, node, deps, disk)
check:
    @echo "Checking remote environment on {{remote_host}}..."
    @echo ""
    @echo "── Connectivity ──"
    @ssh {{remote_host}} "echo 'SSH: ok'" || echo "SSH: FAILED"
    @echo ""
    @echo "── Toolchain ──"
    @ssh {{remote_host}} "{{remote_env}} rustc --version 2>/dev/null || echo 'rustc: NOT FOUND'"
    @ssh {{remote_host}} "{{remote_env}} cargo --version 2>/dev/null || echo 'cargo: NOT FOUND'"
    @ssh {{remote_host}} "node --version 2>/dev/null || echo 'node: NOT FOUND'"
    @ssh {{remote_host}} "npm --version 2>/dev/null || echo 'npm: NOT FOUND'"
    @echo ""
    @echo "── System Libraries ──"
    @ssh {{remote_host}} "pkg-config --modversion opencv4 2>/dev/null && echo '  opencv4: ok' || echo '  opencv4: NOT FOUND'"
    @ssh {{remote_host}} "dpkg -s libclang-dev >/dev/null 2>&1 && echo '  libclang-dev: ok' || echo '  libclang-dev: NOT FOUND'"
    @ssh {{remote_host}} "dpkg -s libudev-dev >/dev/null 2>&1 && echo '  libudev-dev: ok' || echo '  libudev-dev: NOT FOUND'"
    @echo ""
    @echo "── Groups ──"
    @ssh {{remote_host}} "groups"
    @echo ""
    @echo "── Disk ──"
    @ssh {{remote_host}} "df -h /home | tail -1"
    @echo ""
    @echo "── Build Dirs ──"
    @ssh {{remote_host}} "du -sh {{remote_base}}/builds/*/ 2>/dev/null || echo '(none)'"
