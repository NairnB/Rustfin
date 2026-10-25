#!/usr/bin/env bash
# fleet-user.sh create|delete <session> [--admin]   |   sweep --owner <o> [keep-session...]   |   list --legacy
# A throwaway jf-real user "fleet-<owner>-<session>" (owner = env RF_OWNER, required) with a random password. `create` prints {"user":..,"pass":..}.
# Never touches tester/viewer. Callers delete in a trap (fleet-run does); `sweep` clears leftovers of ONE owner
# only; `list --legacy` shows owner-less fleet-* users for a human to decide on (nothing deletes them).
# Optional private settings are read by jfapi.py (see load_env).
exec python3 -I "$(dirname "$(readlink -f "$0")")/jfapi.py" "$@"
