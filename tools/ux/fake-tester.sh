#!/usr/bin/env bash
# Fake tester for dry runs: replays the task's route (one command per line in <routefile>) through the drive
# wrapper and prints the tester's final JSON line. Usage: fake-tester.sh <drive> <session> <routefile>
drive="$1"; session="$2"; route="$3"; n=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  verb="${line%% *}"; rest=""; [ "$verb" != "$line" ] && rest="${line#* }"
  if [ "$verb" = type ]; then "$drive" "$session" type "$rest" || exit 1; else "$drive" "$session" $verb $rest || exit 1; fi
  n=$((n+1))
done < "$route"
"$drive" "$session" wait 3000 >/dev/null; n=$((n+1))
echo "{\"result\":\"done\",\"steps\":$n,\"answer\":\"2006\",\"notes\":\"fake tester replayed the route\",\"screens\":[{\"screen\":\"fake\",\"score\":5,\"why\":\"replay\"}]}"
