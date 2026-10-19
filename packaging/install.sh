#!/bin/sh
# Compatibility wrapper. All privileged setup is implemented in the binary.
set -eu
case "${1-}" in
    --help|-h) printf '%s\n' 'Usage: ./install.sh [--skip-packages]' \
        'Equivalent setup is available directly: ./vimouse init' \
        'No additional packages are installed. Run as your normal desktop user.'; exit 0 ;;
    --skip-packages|'') ;;
    *) printf '%s\n' "Unknown option: $1" >&2; exit 1 ;;
esac
if [ "$(id -u)" -eq 0 ]; then
    printf '%s\n' 'Run ./install.sh as your normal desktop user, without sudo.' >&2
    exit 1
fi
bundle_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
"$bundle_dir/vimouse" init
"$bundle_dir/vimouse" install
printf '%s\n' 'Installed. Run vimouse doctor, then vimouse run.' \
    'Optional login service: vimouse install-service, then vimouse start.'
