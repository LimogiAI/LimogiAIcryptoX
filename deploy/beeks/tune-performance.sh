#!/bin/bash
# Beeks VM Performance Tuning for HFT
# Run this once after VM provisioning

set -e

echo "========================================"
echo "  HFT Performance Tuning"
echo "========================================"

# 1. Network optimizations
echo "[1/4] Applying network optimizations..."
cat >> /etc/sysctl.conf << 'EOF'

# === LimogiAI HFT Network Tuning ===
# Increase socket buffer sizes
net.core.rmem_max=16777216
net.core.wmem_max=16777216
net.core.rmem_default=1048576
net.core.wmem_default=1048576

# TCP buffer sizes
net.ipv4.tcp_rmem=4096 1048576 16777216
net.ipv4.tcp_wmem=4096 1048576 16777216

# Disable Nagle's algorithm for lower latency
net.ipv4.tcp_nodelay=1

# Reduce TCP connection timeouts
net.ipv4.tcp_fin_timeout=15
net.ipv4.tcp_keepalive_time=300
net.ipv4.tcp_keepalive_probes=5
net.ipv4.tcp_keepalive_intvl=15

# Increase connection tracking
net.core.netdev_max_backlog=30000
net.core.somaxconn=65535
net.ipv4.tcp_max_syn_backlog=65535

# Enable TCP Fast Open
net.ipv4.tcp_fastopen=3

# Reduce latency with BBR congestion control (if available)
# net.core.default_qdisc=fq
# net.ipv4.tcp_congestion_control=bbr
EOF

sysctl -p

# 2. Increase file descriptor limits
echo "[2/4] Increasing file descriptor limits..."
cat >> /etc/security/limits.conf << 'EOF'

# === LimogiAI HFT Limits ===
*               soft    nofile          65535
*               hard    nofile          65535
*               soft    nproc           65535
*               hard    nproc           65535
root            soft    nofile          65535
root            hard    nofile          65535
EOF

# 3. Disable unnecessary services
echo "[3/4] Disabling unnecessary services..."
systemctl disable --now snapd.service 2>/dev/null || true
systemctl disable --now snapd.socket 2>/dev/null || true
systemctl disable --now unattended-upgrades 2>/dev/null || true

# 4. CPU governor (if available)
echo "[4/4] Setting CPU governor to performance..."
if [ -f /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor ]; then
    for cpu in /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor; do
        echo "performance" > "$cpu" 2>/dev/null || true
    done
    echo "  CPU governor set to performance"
else
    echo "  CPU governor not available (VM may handle this)"
fi

echo ""
echo "========================================"
echo "  Tuning Complete!"
echo "========================================"
echo ""
echo "Reboot recommended to apply all changes:"
echo "  sudo reboot"
