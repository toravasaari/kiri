#!/bin/bash
# 57-tmp-size — cap /tmp at 2 GiB.
# Installs: /etc/systemd/system/tmp.mount.d/size.conf
#
# Ubuntu mounts /tmp as a tmpfs of half the host's memory, but its pages
# count against the container's memory limit and cannot be swapped out, so
# a full /tmp makes the kernel kill processes. With the cap, a write fails
# with "no space left on device" instead. The other options are systemd's.
set -euo pipefail

echo "==> Capping /tmp at 2 GiB"
mkdir -p /etc/systemd/system/tmp.mount.d
cat > /etc/systemd/system/tmp.mount.d/size.conf <<'CONF'
[Mount]
Options=mode=1777,strictatime,nosuid,nodev,size=2G,nr_inodes=1m,x-systemd.graceful-option=usrquota
CONF
