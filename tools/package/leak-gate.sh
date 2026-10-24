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
# the remote's colour keys (Key::G…, its script name and the TV key name) are not the surname
THIRD_PARTY="$THIRD_PARTY|Key::G[r]een|Key\\(G[r]een|\"g[r]een\" =>|key g[r]een|red.g[r]een.yellow|ColorF1G[r]een|^ +G[r]een,$"
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
    WORDS="$WORDS|b[o]nanza|h[i]s girl friday|living d[e]ad|the g[e]neral \\(?1926"
    # Private words (a look and names that must never go public) live outside the repo so this file names none of
    # them. Line 1: extra word regex; line 2 (optional): exact third-party strings to strip first; line 3 (optional):
    # one sample word the list must catch. Fails closed.
    PRIV=${RF_PRIVATE_WORDS:-$HOME/.config/rf-private-words}
    if [ ! -s "$PRIV" ]; then echo "leak gate (public): private word list missing ($PRIV)" >&2; exit 1; fi
    W1=$(sed -n 1p "$PRIV" | tr -d '\r' | sed 's/^\xEF\xBB\xBF//'); P2=$(sed -n 2p "$PRIV" | tr -d '\r')
    C3=$(sed -n 3p "$PRIV" | tr -d '\r')   # line 3 (optional): one sample private word, no spaces, that must be caught (canary)
    [ -n "$W1" ] || { echo "leak gate (public): private word list line 1 empty" >&2; exit 1; }
    WORDS="$WORDS|$W1"
    # line 2 is literal strings split on '|': escape them so a stray '.', '/' or '*' can never strip real text
    if [ -n "$P2" ]; then
      THIRD_PARTY="$THIRD_PARTY|$(printf '%s' "$P2" | sed -E 's/[][\\.*^$+?(){}\/]/\\&/g')"
    fi ;;
  *) echo "mode must be plain, preset or public" >&2; exit 2 ;;
esac
# A broken pattern or a missing tool must fail the gate, never pass it silently.
command -v strings >/dev/null || { echo "leak gate: 'strings' missing (binutils) — cannot check binary files" >&2; exit 1; }
rc=0; printf 'x\n' | grep -aiE "($WORDS|$TAILNET)" >/dev/null || rc=$?
[ $rc -le 1 ] || { echo "leak gate: word pattern does not compile" >&2; exit 1; }
# Canary: a pattern that compiles but closes a group early (a stray ')') can switch other checks off.
for c in 1''00.64.0.1 /h''ome/ ${C3:-}; do
  printf '%s\n' "$c" | grep -qaiE "($WORDS|$TAILNET)" || { echo "leak gate: canary '$c' not caught — pattern broken" >&2; exit 1; }
done
printf 'x\n' | sed -E "s/$THIRD_PARTY//g" >/dev/null || { echo "leak gate: strip pattern does not compile" >&2; exit 1; }
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
      check "$rel" < <(strings -n 6 "$f"; strings -n 6 -e l "$f") || hits=1 ;;
    *)
      if file -b --mime-type "$f" | grep -qE '^text/|xml|json|empty'; then check "$rel" < "$f" || hits=1
      else check "$rel" < <(strings -n 6 "$f"; strings -n 6 -e l "$f") || hits=1; fi ;;   # + UTF-16LE text
  esac
  check "file name $rel" < <(printf '%s\n' "$rel") || hits=1
done < <(find "$DIR" -type f -print0)
n=$(find "$DIR" -type f | wc -l)
if [ $hits -eq 0 ]; then echo "leak gate ($MODE): clean — $n files"; else echo "leak gate ($MODE): FAILED"; exit 1; fi
