#!/usr/bin/env sh
# tedit-rust updater — POSIX sh, git-aware, cargo build

set -eu

APP_NAME="tedit-rust"
BIN_NAME="tedit-rust"
CARGO_BIN="trust"
DEFAULT_PREFIX="/usr/local"
USER_PREFIX="$HOME/.local"
LOG="$(mktemp -t ${APP_NAME}-update.XXXXXX.log)"

SCRIPT_DIR=$(
  CDPATH= cd -P -- "$(dirname -- "$0")" 2>/dev/null && pwd
)

# smart repo discovery (new first, then old crab name as fallback)
REPO_DIR="${TEDIT_RUST_REPO:-}"
[ -z "$REPO_DIR" ] && [ -f "$PWD/Cargo.toml" ] && REPO_DIR="$PWD"
[ -z "$REPO_DIR" ] && [ -f "$SCRIPT_DIR/Cargo.toml" ] && REPO_DIR="$SCRIPT_DIR"
[ -z "$REPO_DIR" ] && [ -f "/usr/local/share/${APP_NAME}/repo" ] && RD="$(cat /usr/local/share/${APP_NAME}/repo 2>/dev/null || true)" && [ -n "$RD" ] && [ -f "$RD/Cargo.toml" ] && REPO_DIR="$RD"
# compat with old name
[ -z "$REPO_DIR" ] && [ -f "/usr/local/share/tedit-crab/repo" ] && RD="$(cat /usr/local/share/tedit-crab/repo 2>/dev/null || true)" && [ -n "$RD" ] && [ -f "$RD/Cargo.toml" ] && REPO_DIR="$RD"
[ -z "$REPO_DIR" ] && [ -f "$HOME/${APP_NAME}/Cargo.toml" ] && REPO_DIR="$HOME/${APP_NAME}"

[ -n "$REPO_DIR" ] || { echo "ERROR: Could not locate the ${APP_NAME} repo. Set TEDIT_RUST_REPO=/path/to/repo" >&2; exit 2; }

cd "$REPO_DIR" || { echo "ERROR: cannot cd to $REPO_DIR" >&2; exit 2; }

if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
  GREEN="$(tput setaf 2)"; YELLOW="$(tput setaf 3)"; RED="$(tput setaf 1)"; CYAN="$(tput setaf 6)"
  BOLD="$(tput bold)"; RESET="$(tput sgr0)"
else
  GREEN=""; YELLOW=""; RED=""; CYAN=""; BOLD=""; RESET=""
fi

say() { printf "%s\n" "$*" | tee -a "$LOG" >/dev/null; }
die(){ finish_ui 2>/dev/null || :; printf "%sERROR:%s %s\n" "$RED" "$RESET" "$1" | tee -a "$LOG" >&2; exit 1; }
have(){ command -v "$1" >/dev/null 2>&1; }

is_utf(){ echo "${LC_ALL:-${LANG:-}}" | grep -qi 'utf-8'; }
repeat(){ n=$1; ch=$2; i=0; out=""; while [ "$i" -lt "$n" ]; do out="$out$ch"; i=$((i+1)); done; printf "%s" "$out"; }
term_width(){ w=80; if command -v tput >/dev/null 2>&1; then w=$(tput cols 2>/dev/null || echo 80); fi; [ "$w" -gt 24 ] || w=80; echo "$w"; }

if is_utf; then FIL="█"; EMP="░"; else FIL="#"; EMP="-"; fi
TOTAL=11
STEP=0

draw_bar(){ n=$1; tot=$2; msg=$3; tw=$(term_width); bw=$(( tw - 32 )); [ "$bw" -lt 12 ] && bw=12; [ "$bw" -gt 60 ] && bw=60; [ "$tot" -gt 0 ] || tot=1; pct=$(( n*100 / tot )); fill=$(( n*bw / tot )); empty=$(( bw - fill )); bar="$(repeat "$fill" "$FIL")$(repeat "$empty" "$EMP")"; printf "\r\033[K%s%s[%s]%s %d/%d (%d%%) %s" "$CYAN" "$BOLD" "$bar" "$RESET" "$n" "$tot" "$pct" "$msg"; printf "[%s] %d/%d (%d%%) %s\n" "$(repeat "$fill" "#")$(repeat "$empty" "-")" "$n" "$tot" "$pct" "$msg" >>"$LOG"; }
next(){ STEP=$((STEP+1)); [ "$STEP" -gt "$TOTAL" ] && STEP="$TOTAL"; draw_bar "$STEP" "$TOTAL" "$1"; }

spinner(){
  msg="$1"; shift
  if [ "${VERBOSE-0}" = "1" ] || [ ! -t 1 ]; then
    sh -c "$*" 2>&1 | tee -a "$LOG"
    return $?
  fi
  sh -c "$*" >>"$LOG" 2>&1 &
  pid=$!
  frames='-\|/.'; i=0
  while kill -0 "$pid" 2>/dev/null; do i=$(( (i+1) % 4 )); c=$(printf %s "$frames" | cut -c $((i+1))); printf "\r\033[K[%s] %s" "$c" "$msg"; sleep 0.1; done
  wait "$pid" 2>/dev/null || true
  printf "\r\033[K"
}

CURSOR_HIDE=""; CURSOR_SHOW=""
if command -v tput >/dev/null 2>&1; then
  CURSOR_HIDE="$(tput civis 2>/dev/null || true)"
  CURSOR_SHOW="$(tput cnorm 2>/dev/null || true)"
fi
printf "%s" "$CURSOR_HIDE"
finish_ui(){ printf "\r\033[K\n%s" "$CURSOR_SHOW"; }
trap 'finish_ui' EXIT INT TERM

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if have sudo; then SUDO="sudo"
  elif have doas; then SUDO="doas"
  fi
fi

auth_once(){
  [ -z "$SUDO" ] && return 0
  printf "\r\033[K%s%sElevating privileges (may prompt once)...%s\n" "$CYAN" "$BOLD" "$RESET"
  if [ "$SUDO" = "sudo" ]; then sudo -v; else $SUDO true; fi
}
run_root(){
  msg="$1"; cmd="$2"
  if [ -z "$SUDO" ]; then spinner "$msg" "$cmd"; return; fi
  if $SUDO -n true 2>/dev/null; then spinner "$msg" "$SUDO $cmd"
  else
    printf "\r\033[K%s%s%s %s\n" "$YELLOW" "[auth]" "$RESET" "$msg"
    $SUDO sh -c "$cmd" 2>&1 | tee -a "$LOG"
    draw_bar "$STEP" "$TOTAL" "$msg"
  fi
}

printf "%s%s>> Updating %s <<%s\n" "$CYAN" "$BOLD" "$APP_NAME" "$RESET" | tee -a "$LOG" >/dev/null

next "Locating repository"
[ -f "Cargo.toml" ] || die "This does not look like the ${APP_NAME} repo."

next "Checking for git"
IN_GIT=0; if have git && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then IN_GIT=1; fi

next "Fetching latest (if git)"
if [ "$IN_GIT" -eq 1 ]; then
  spinner "git fetch..." "GIT_TERMINAL_PROMPT=0 git fetch --all --tags --prune || true"
  LOCAL=$(git rev-parse HEAD 2>/dev/null || echo "")
  UPSTREAM=$(git rev-parse '@{u}' 2>/dev/null || echo "")
  if [ -n "$UPSTREAM" ] && [ "$LOCAL" = "$UPSTREAM" ]; then
    finish_ui
    echo "Already up to date."
    echo "Log: $LOG"
    exit 0
  fi
  spinner "git pull..." "GIT_TERMINAL_PROMPT=0 git pull --rebase --autostash || git pull || true"
else
  printf "Not a git repo; just rebuilding.\n" >>"$LOG"
fi

next "Preparing privileges"
auth_once
draw_bar "$STEP" "$TOTAL" "Preparing privileges"

next "Checking Rust toolchain"
if ! have cargo || ! have rustc; then
  die "Rust toolchain not found. Install rustc + cargo, then re-run."
fi

next "Rebuilding (release)"
spinner "cargo build --release ..." "cargo build --release"

TARGET_BIN="./target/release/$CARGO_BIN"
[ -x "$TARGET_BIN" ] || {
  ALT="./target/release/$BIN_NAME"
  [ -x "$ALT" ] || die "Build succeeded but neither $TARGET_BIN nor $ALT found."
  TARGET_BIN="$ALT"
}

next "Installing"
TARGET_PREFIX="$DEFAULT_PREFIX"
if [ "$(id -u)" -ne 0 ] && [ -z "$SUDO" ]; then
  TARGET_PREFIX="$USER_PREFIX"
  spinner "Installing (user)..." "install -m 755 '$TARGET_BIN' '$TARGET_PREFIX/bin/$BIN_NAME'"
else
  run_root "Installing (system)..." "install -m 755 '$TARGET_BIN' '$TARGET_PREFIX/bin/$BIN_NAME'"
fi

next "Stripping (optional)"
if have strip; then
  BIN_PATH="${TARGET_PREFIX}/bin/${BIN_NAME}"
  [ -e "$BIN_PATH" ] && spinner "Stripping..." "${SUDO:+$SUDO }strip '$BIN_PATH' 2>/dev/null || true"
fi

next "Done"
finish_ui
printf "%s%s✅ %s updated successfully.%s\n" "$GREEN" "$BOLD" "$APP_NAME" "$RESET"
printf "Log: %s\n" "$LOG"
