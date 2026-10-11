#!/usr/bin/env bash
# Leak gate: fails (exit 1) if a package or a source export carries personal data, private addresses or secrets.
#   tools/package/leak-gate.sh <dir> plain|preset|public
#   plain   a build with no preset server: nothing personal at all
#   preset  a build with a server baked in: only the address in $PRESET_SERVER is allowed
#   public  the public source export: a longer word list plus secret/key patterns
# Our binaries are read with `strings -n 6`, text files as they are; case-insensitive substring match.
# Bundled third-party libraries (libmpv*.dll, *.dylib) are only checked for build-machine home paths.
# The word lists below are written with a bracket inside each word (n[a]me) so this file does not match itself.
set -euo pipefail
DIR=$1 MODE=$2
# The surname and machine words match as whole words only (letters on either side = part of another word, e.g. ...ByRequest).
# Portable form (BSD grep on the Mac has no reliable \b): a non-letter or line edge on each side.
WORDS='n[a]irn|(^|[^a-z])b[r]own([^a-z]|$)|(^|[^a-z])b[y]re([^a-z]|$)|h[a]mestack|(^|[^a-z])f[a]rm([^a-z]|$)|192\.1[6]8\.|/h[o]me/|g[m]ail|h[o]tmail|w[i]llie|n[i]lk'
TAILNET='(^|[^0-9])1[0]0\.(6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])\.[0-9]{1,3}\.[0-9]{1,3}'   # carrier-grade NAT range (/10), used by tailnets
HOMEPATH='/h[o]me/n[a]irn|/U[s]ers/n[a]irn'   # the build machine's home (third-party libs carry other CI paths)
# Exact third-party names in our binaries or Cargo.lock that contain a listed word (each checked by hand):
# a Rust hash-map crate, a TV-remote key name in winit, an OpenGL function, a macOS AppKit selector,
# a glyph name in the Inter font (core/assets/fonts).
THIRD_PARTY='hashb[r]own|ColorF5B[r]own|ByR[e]gion|initByR[e]ferencingFile|kg[r]eenlandic'
case "$MODE" in
  plain) ALLOW='^$' ;;
  preset)
    ALLOW="${PRESET_SERVER:?preset mode needs PRESET_SERVER}"
    ALLOW=${ALLOW//./\\.} ;;
  public)
    ALLOW='^$'
    WORDS="$WORDS|l[e]wis|g[r]een|c[r]aig|d[o]uglas|m[a]rjory|l[i]am|c[a]rys|a[t]hena|o[p]encode|d[e]epseek|s[e]at"
    WORDS="$WORDS|t[a]ilscale|ts\.n[e]t|d[u]id|3[e]bcf9d0"
    WORDS="$WORDS|-----B[E]GIN|g[h]p_|s[k]-[A-Za-z0-9]{8}|A[K]IA[A-Z0-9]{8}|T[U]NNEL_TOKEN"
    # jf-real's public-domain titles: the films are free but their TMDB posters/backdrops may not be, so public
    # files may show only the Blender open movies (CC BY) or jf-test titles (pit crew, 11 Oct 2026).
    WORDS="$WORDS|b[o]nanza|h[i]s girl friday|living d[e]ad|the g[e]neral \\(?1926" ;;
  *) echo "mode must be plain, preset or public" >&2; exit 2 ;;
esac
hits=0
check() { # <label> ; text on stdin ; returns 1 on a hit
  local found
  found=$(sed -E "s/$THIRD_PARTY//g" | grep -aioE ".{0,30}($WORDS|$TAILNET).{0,30}" | grep -avE "$ALLOW" | sort -u | head -5 || true)
  [ -z "$found" ] && return 0
  echo "LEAK in $1:"; echo "$found" | sed 's/^/    /'
  return 1
}
while IFS= read -r -d '' f; do
  rel=${f#"$DIR"/}
  case "$rel" in
    *libmpv*.dll|*.dylib)
      if strings -n 6 "$f" | grep -aqE "$HOMEPATH"; then echo "LEAK in $rel: build-machine home path"; hits=1; fi ;;
    *.exe|*/MacOS/*|rustfin|jellyfin-desktop)
      check "$rel" < <(strings -n 6 "$f") || hits=1 ;;
    *)
      if file -b --mime-type "$f" | grep -qE '^text/|xml|json|empty'; then check "$rel" < "$f" || hits=1
      else check "$rel" < <(strings -n 6 "$f") || hits=1; fi ;;
  esac
  check "file name $rel" < <(printf '%s\n' "$rel") || hits=1
done < <(find "$DIR" -type f -print0)
n=$(find "$DIR" -type f | wc -l)
if [ $hits -eq 0 ]; then echo "leak gate ($MODE): clean — $n files"; else echo "leak gate ($MODE): FAILED"; exit 1; fi
