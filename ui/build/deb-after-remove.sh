#!/bin/sh
# No-op: capabilities live on the binary's inode and are removed along with
# the file itself during uninstall. Kept for symmetry with afterInstall.
exit 0
