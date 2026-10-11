#!/usr/bin/env bash
# fleet-user.sh create|delete <session> [--admin]   |   sweep [keep-session...]
# A throwaway jf-real user "fleet-<session>" with a random password. `create` prints {"user":..,"pass":..}.
# Never touches tester/viewer. Callers delete in a trap (fleet-run does); `sweep` clears leftovers.
# Optional private settings are read by jfapi.py (see load_env).
exec python3 -I "$(dirname "$(readlink -f "$0")")/jfapi.py" "$@"
