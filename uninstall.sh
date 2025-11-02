#!/usr/bin/env sh
# tedit-rust uninstaller — POSIX, progress bar, removes binary, wrappers, user data
# also cleans up old tedit-crab + trust names for backward compat

set -u

APP_NAME="tedit-rust"
LOG="/tmp/${APP_NAME}-uninstall.$$".log

PURGE_REPO=0
PURGE_USER=0
ASSUME_YES=0

while [ $# -gt 0 ]; do
  case "$1" in
    --purge-repo)      PURGE_REPO=1 ;;
    --purge-user-data) PURGE_USER=1 ;;
    --purge)           PURGE_REPO=1; PURGE_USER=1 ;;
    -y|--yes)          ASSUME_YES=1 ;;
    -h|--help)
      cat <<EOF
Usage: $0 [options]
  --purge-repo        Remove this git repository after uninstall
  --purge-user-data   Remove ~/.tedit-rust*, ~/.config/tedit-rust
  --purge             Both
  -y, --yes           Non-interactive
EOF
      exit 0
      ;;
    *) printf "Unknown option: %s\n" "$1" >&2; exit 2 ;;
  esac
  shift
done

if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
  GREEN="$(tput setaf 2)"; YELLOW="$(tput setaf 3)"; RED="$(tput setaf 1)"; CYAN="$(tput setaf 6)"; BOLD="$(tput bold)"; RESET="$(tput sgr0)"
else
  GREEN=""; YELLOW=""; RED=""; CYAN=""; BOLD=""; RESET=""
fi

log(){ printf "%s\n" "$*" >>"$LOG"; }

SCRIPT_DIR=$(
  CDPATH= cd -P -- "$(dirname -- "$0")" 2>/dev/null && pwd
)

have(){ command -v "$1" >/dev/null 2>&1; }

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if have doas; then SUDO="doas"
  elif have sudo; then SUDO="sudo"
  fi
fi

if [ -n "$SUDO" ]; then
  printf "%s%sUninstalling %s... (authenticating)%s\n" "$CYAN" "$BOLD" "$APP_NAME" "$RESET"
  $SUDO -v 2>/dev/null || $SUDO true || :
else
  printf "%s%sUninstalling %s...%s\n" "$CYAN" "$BOLD" "$APP_NAME" "$RESET"
fi

SUDO_NONINT=""
[ -n "$SUDO" ] && SUDO_NONINT="$SUDO -n"

is_utf(){ echo "${LC_ALL:-${LANG:-}}" | grep -qi 'utf-8'; }
if is_utf; then FIL="█"; EMP="░"; else FIL="#"; EMP="-"; fi
repeat(){ n=$1; ch=$2; i=0; out=""; while [ "$i" -lt "$n" ]; do out="$out$ch"; i=$((i+1)); done; printf "%s" "$out"; }
term_width(){ w=80; if command -v tput >/dev/null 2>&1; then w=$(tput cols 2>/dev/null || echo 80); fi; [ "$w" -gt 20 ] || w=80; echo "$w"; }

CURSOR_HIDE=""; CURSOR_SHOW=""
if command -v tput >/dev/null 2>&1; then
  CURSOR_HIDE="$(tput civis 2>/dev/null || true)"
  CURSOR_SHOW="$(tput cnorm 2>/dev/null || true)"
fi
printf "%s" "$CURSOR_HIDE"
finish_bar(){ printf "\r\033[K\n%s" "$CURSOR_SHOW"; }
trap 'finish_bar' EXIT INT TERM

draw_bar(){
  n=$1 tot=$2 msg=$3
  tw=$(term_width)
  [ "$tot" -gt 0 ] || tot=1
  bw=$(( tw - 32 )); [ "$bw" -lt 10 ] && bw=10; [ "$bw" -gt 60 ] && bw=60
  pct=$(( n*100 / tot ))
  fill=$(( n*bw / tot ))
  empty=$(( bw - fill ))
  bar="$(repeat "$fill" "$FIL")$(repeat "$empty" "$EMP")"
  printf "\r\033[K%s%s[%s]%s %d/%d (%d%%) %s" "$CYAN" "$BOLD" "$bar" "$RESET" "$n" "$tot" "$pct" "$msg"
  log "$(printf '[%s] %d/%d (%d%%) %s' "$(repeat "$fill" "#")$(repeat "$empty" "-")" "$n" "$tot" "$pct" "$msg")"
}

SUDO_RUN(){
  if [ -n "$SUDO" ]; then
    $SUDO_NONINT "$@" 2>/dev/null || $SUDO "$@" 2>/dev/null || :
  else
    "$@" 2>/dev/null || :
  fi
}

rm_file(){
  P="$1"; [ -e "$P" ] || return 0
  case "$P" in
    /usr/*|/etc/*|/opt/*|/var/*) SUDO_RUN rm -f -- "$P" ;;
    *) rm -f -- "$P" 2>/dev/null || : ;;
  esac
}

rm_dir(){
  D="$1"; [ -d "$D" ] || return 0
  case "$D" in
    /usr/*|/etc/*|/opt/*|/var/*) SUDO_RUN rm -rf -- "$D" ;;
    *) rm -rf -- "$D" 2>/dev/null || : ;;
  esac
}

remove_named_everywhere(){
  NAME="$1"
  for D in /usr/local/bin /usr/bin "$HOME/.local/bin"; do
    [ -f "$D/$NAME" ] && rm_file "$D/$NAME"
  done
  OLDIFS=$IFS; IFS=:
  for D in $PATH; do [ -n "$D" ] && [ -f "$D/$NAME" ] && rm_file "$D/$NAME"; done
  IFS=$OLDIFS
  RES="$(command -v "$NAME" 2>/dev/null || true)"
  [ -n "$RES" ] && [ -f "$RES" ] && rm_file "$RES"
  hash -r 2>/dev/null || :
}

confirm(){
  [ "$ASSUME_YES" -eq 1 ] && return 0
  finish_bar
  printf "%sRemove %s?%s [y/N] " "$BOLD" "$1" "$RESET" 1>&2
  read -r A || A=""
  case "$A" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

TOTAL=6
[ "$PURGE_USER" -eq 1 ] && TOTAL=$((TOTAL+1))
[ "$PURGE_REPO" -eq 1 ] && TOTAL=$((TOTAL+1))

STEP=0
next(){ STEP=$((STEP+1)); draw_bar "$STEP" "$TOTAL" "$1"; }

# 1) main binary (new + compat names)
remove_named_everywhere "$APP_NAME"
remove_named_everywhere "trust"
remove_named_everywhere "tedit-crab"
next "Remove binaries"

# 2) wrappers (new names)
for W in "${APP_NAME}-install" "${APP_NAME}-update" "${APP_NAME}-uninstall"; do
  remove_named_everywhere "$W"
done
# compat old wrappers
for W in "tedit-crab-install" "tedit-crab-update" "tedit-crab-uninstall"; do
  remove_named_everywhere "$W"
done
next "Remove wrappers"

# 3) man
for MP in "/usr/local/share/man/man1/${APP_NAME}.1" "/usr/share/man/man1/${APP_NAME}.1" "$HOME/.local/share/man/man1/${APP_NAME}.1"; do
  rm_file "$MP"; rm_file "${MP}.gz"
done
# compat: old man
for MP in "/usr/local/share/man/man1/tedit-crab.1" "/usr/share/man/man1/tedit-crab.1" "$HOME/.local/share/man/man1/tedit-crab.1"; do
  rm_file "$MP"; rm_file "${MP}.gz"
done
next "Remove man pages"

# 4) repo note
rm_file "/usr/local/share/${APP_NAME}/repo"
rm_dir  "/usr/local/share/${APP_NAME}"
# compat
rm_file "/usr/local/share/tedit-crab/repo"
rm_dir  "/usr/local/share/tedit-crab"
next "Remove repo markers"

# 5) purge user data
if [ "$PURGE_USER" -eq 1 ]; then
  rm -f "$HOME/.${APP_NAME}rc" 2>/dev/null || :
  rm -rf "$HOME/.${APP_NAME}" 2>/dev/null || :
  rm -rf "$HOME/.config/${APP_NAME}" 2>/dev/null || :
  # compat
  rm -f "$HOME/.tedit-crabrc" 2>/dev/null || :
  rm -rf "$HOME/.tedit-crab" 2>/dev/null || :
  rm -rf "$HOME/.config/tedit-crab" 2>/dev/null || :
  next "Purge user data"
fi

# 6) post check
next "Post-check"
finish_bar
printf "%s%s✅ %s uninstalled.%s\n" "$GREEN" "$BOLD" "$APP_NAME" "$RESET"
printf "Log: %s\n" "$LOG"

# 7) purge repo
if [ "$PURGE_REPO" -eq 1 ]; then
  if [ -d "$SCRIPT_DIR/.git" ] && confirm "Delete repo at $SCRIPT_DIR"; then
    PARENT=$(dirname "$SCRIPT_DIR")
    ( cd "$PARENT" 2>/dev/null && rm -rf "$(basename "$SCRIPT_DIR")" )
    echo "Repo removed."
  else
    echo "Skipped repo removal."
  fi
fi
