#!/usr/bin/env sh
# tedit-rust PATH initializer (POSIX sh)
# installs wrappers: tedit-rust-install / tedit-rust-update / tedit-rust-uninstall

set -eu

APP_NAME="tedit-rust"

REPO_DIR=$(
  CDPATH= cd -P -- "$(dirname -- "$0")" 2>/dev/null && pwd
)

[ -f "$REPO_DIR/Cargo.toml" ] || {
  printf "ERROR: Cargo.toml not found in %s (are you in the repo root?)\n" "$REPO_DIR" >&2
  exit 1
}

if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
  GREEN="$(tput setaf 2)"; YELLOW="$(tput setaf 3)"; CYAN="$(tput setaf 6)"; BOLD="$(tput bold)"; RESET="$(tput sgr0)"
else
  GREEN=""; YELLOW=""; CYAN=""; BOLD=""; RESET=""
fi

say(){ printf "%s\n" "$*"; }
warn(){ printf "%sWARNING:%s %s\n" "$YELLOW" "$RESET" "$*" >&2; }
die(){ printf "%sERROR:%s %s\n" "$YELLOW" "$RESET" "$*" >&2; exit 1; }
have(){ command -v "$1" >/dev/null 2>&1; }

SYSTEM=0
[ "${1-}" = "--system" ] && SYSTEM=1

if [ "$(id -u)" -eq 0 ] && [ "$SYSTEM" -eq 0 ]; then
  SYSTEM=1
fi

DEST_BIN="$HOME/.local/bin"
SUDO=""
if [ "$SYSTEM" -eq 1 ]; then
  DEST_BIN="/usr/local/bin"
  if [ "$(id -u)" -ne 0 ]; then
    if   have sudo; then SUDO="sudo"
    elif have doas; then SUDO="doas"
    else die "Need sudo/doas to write to $DEST_BIN (or re-run without --system)."
    fi
  fi
fi

if [ "$SYSTEM" -eq 1 ]; then
  ${SUDO:+$SUDO }mkdir -p "$DEST_BIN"
else
  mkdir -p "$DEST_BIN"
fi

sq(){ printf %s "$1" | sed "s/'/'\"'\"'/g; s/^/'/; s/\$/'/"; }

make_wrapper(){
  name="$1"
  target="$2"
  tmp="${DEST_BIN}/.${name}.$$"
  REPO_ESC="$(sq "$REPO_DIR")"
  TARGET_ESC="$(sq "./$target")"

  {
    printf '#!/usr/bin/env sh\n'
    printf 'set -eu\n'
    printf 'REPO_DIR=%s\n' "$REPO_ESC"
    printf 'TARGET=%s\n' "$TARGET_ESC"
    printf 'if [ ! -f "$REPO_DIR/Cargo.toml" ]; then\n'
    printf '  echo "ERROR: %s repo not found at $REPO_DIR." >&2\n' "$APP_NAME"
    printf '  echo "Hint: re-clone and re-run: sh init.sh" >&2\n'
    printf '  exit 1\n'
    printf 'fi\n'
    printf 'cd -- "$REPO_DIR"\n'
    printf 'exec sh "$TARGET" "$@"\n'
  } > "$tmp"

  if [ "$SYSTEM" -eq 1 ]; then
    ${SUDO:+$SUDO }mv -f "$tmp" "$DEST_BIN/$name"
    ${SUDO:+$SUDO }chmod 0755 "$DEST_BIN/$name"
  else
    mv -f "$tmp" "$DEST_BIN/$name"
    chmod 0755 "$DEST_BIN/$name"
  fi

  say "Installed wrapper: $DEST_BIN/$name -> $target"
}

[ -f "$REPO_DIR/install.sh" ]   || die "install.sh not found in repo."
[ -f "$REPO_DIR/update.sh" ]    || die "update.sh not found in repo."
[ -f "$REPO_DIR/uninstall.sh" ] || die "uninstall.sh not found in repo."

make_wrapper "${APP_NAME}-install"   "install.sh"
make_wrapper "${APP_NAME}-update"    "update.sh"
make_wrapper "${APP_NAME}-uninstall" "uninstall.sh"

if [ "$SYSTEM" -eq 0 ]; then
  case ":$PATH:" in
    *:"$DEST_BIN":*) in_path=1 ;;
    *) in_path=0 ;;
  esac
  if [ $in_path -eq 0 ]; then
    profile=""
    [ -n "${ZDOTDIR-}" ] && [ -w "${ZDOTDIR}/.zprofile" 2>/dev/null ] && profile="${ZDOTDIR}/.zprofile"
    [ -z "$profile" ] && [ -w "$HOME/.zprofile" 2>/dev/null ] && profile="$HOME/.zprofile"
    [ -z "$profile" ] && [ -w "$HOME/.bash_profile" 2>/dev/null ] && profile="$HOME/.bash_profile"
    [ -z "$profile" ] && profile="$HOME/.profile"

    mkdir -p "$(dirname "$profile")" 2>/dev/null || true
    if ! grep -F "$DEST_BIN" "$profile" >/dev/null 2>&1; then
      printf '\n# Added by %s init\nexport PATH="%s:$PATH"\n' "$APP_NAME" "$DEST_BIN" >> "$profile"
      say "${YELLOW}Added ${DEST_BIN} to PATH in ${profile}.${RESET}"
    fi
    say "Run: ${BOLD}export PATH=\"${DEST_BIN}:\$PATH\"${RESET}"
  fi
fi

printf "%s%s✅ %s wrappers ready.%s\n" "$GREEN" "$BOLD" "$APP_NAME" "$RESET"
say "Use from anywhere:"
say "  ${BOLD}${APP_NAME}-install${RESET}"
say "  ${BOLD}${APP_NAME}-update${RESET}"
say "  ${BOLD}${APP_NAME}-uninstall${RESET}"
