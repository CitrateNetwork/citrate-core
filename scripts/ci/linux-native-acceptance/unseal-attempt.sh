#!/usr/bin/env bash
# created: 2026-10-27 | branch: ci/k1l-linux-native-acceptance | author: Larry Klosowski + Claude Opus 5.5 (DGX lane)
# status: active (K1-L). PROBE FIXTURE ONLY: never shipped, never part of the app.
#
# Started by linux-native-acceptance.sh after the seal, through the same launcher as the app
# (`"${CONFINE[@]}"`), before the first launch. It tries to undo the seal from where the app
# would run, using mount operations only (it writes no file):
#   remount the root and the harness copy read-write (plain and bind remount),
#   clear the read-only flag with mount_setattr(2) on the root (recursive) and the harness copy,
#   unmount the tmpfs over /home (plain and lazy) and the read-only harness bind (lazy),
#   enter PID 1's mount namespace (nsenter -t 1 -m).
# Each attempt must be refused. It also reports its effective capabilities, no_new_privs and its
# user and mount namespaces, which the harness compares with PID 1's. One line per attempt:
#   attempt <name>: refused|SUCCEEDED
# Exit 0 only if every attempt was refused.
set -u
H=/mnt/work/harness
ok=0
attempt() { # name, then the command
  local name="$1"; shift
  if "$@" >/dev/null 2>&1; then echo "attempt $name: SUCCEEDED"; ok=1; else echo "attempt $name: refused"; fi
}
clear_ro() { # path [-r]: mount_setattr(2) clearing MOUNT_ATTR_RDONLY
  python3 -I - "$@" <<'PY'
import ctypes, sys
class MountAttr(ctypes.Structure):
    _fields_ = [("attr_set", ctypes.c_uint64), ("attr_clr", ctypes.c_uint64),
                ("propagation", ctypes.c_uint64), ("userns_fd", ctypes.c_uint64)]
attr = MountAttr(0, 0x1, 0, 0)  # attr_clr = MOUNT_ATTR_RDONLY
flags = 0x8000 if sys.argv[2:] == ["-r"] else 0  # AT_RECURSIVE
libc = ctypes.CDLL(None, use_errno=True)
rc = libc.syscall(ctypes.c_long(442), ctypes.c_int(-100), sys.argv[1].encode(), ctypes.c_uint(flags),
                  ctypes.byref(attr), ctypes.c_size_t(ctypes.sizeof(attr)))  # 442 = mount_setattr
sys.exit(0 if rc == 0 else 1)
PY
}
echo "caps: $(awk '/^CapEff:/ {print $2}' /proc/self/status) no_new_privs: $(awk '/^NoNewPrivs:/ {print $2}' /proc/self/status)"
echo "ns: user $(readlink /proc/self/ns/user) mnt $(readlink /proc/self/ns/mnt)"
attempt "remount,rw /" mount -o remount,rw /
attempt "remount,bind,rw /" mount -o remount,bind,rw /
attempt "remount,bind,rw $H" mount -o remount,bind,rw "$H"
attempt "mount_setattr clear rdonly / (recursive)" clear_ro / -r
attempt "mount_setattr clear rdonly $H" clear_ro "$H"
attempt "umount /home" umount /home
attempt "umount -l /home" umount -l /home
attempt "umount -l $H" umount -l "$H"
attempt "nsenter -t 1 -m" nsenter -t 1 -m true
exit "$ok"
