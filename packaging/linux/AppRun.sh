#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
#
# The AppImage's entry point (P4-18), installed as `AppRun`: the app from the AppImage's own
# folder, where the preview host is beside it, or `quakec-ide` or `quakec-lsp` when named first.
here=$(dirname "$(readlink -f "$0")")
case "${1:-}" in
quakec-ide | quakec-lsp)
  command=$1
  shift
  exec "$here/usr/lib/quakec-studio/$command" "$@"
  ;;
esac
exec "$here/usr/lib/quakec-studio/quakec-studio" "$@"
