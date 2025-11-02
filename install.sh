#!/usr/bin/env sh
# tedit-rust installer (POSIX sh) — cinematic bar, Rust-aware
# builds via cargo and installs to /usr/local/bin/tedit-rust (or ~/.local/bin)

set -eu

APP_NAME="tedit-rust"
BIN_NAME="tedit-rust"     # name we INSTALL as
CARGO_BIN="trust"         # name cargo actually builds (your crate)
DEFAULT_PREFIX="/usr/local"
USER_PREFIX="${PREFIX:-$HOME/.local}"
LOG="$(mktemp -t ${APP_NAME}-install.XXXXXX.log)"

SCRIPT_DIR=$(
  CDPATH= cd -P -- "$(dirname -- "$0")" 2>/dev/null && pwd
)
cd "$SCRIPT_DIR" || {
  echo "ERROR: cannot cd to $SCRIPT_DIR" >&2
  exit 2
}

[ -f "Cargo.toml" ] || {
  echo "ERROR: Cargo.toml not found (run from the ${APP_NAME} source tree)" >&2
  exit 2
}
[ -d "src" ] || mkdir -p src

if [ -t 1 ] && command -v tput >/dev/null 2>&1; then
  GREEN="$(tput setaf 2)"; YELLOW="$(tput setaf 3)"; RED="$(tput setaf 1)"; CYAN="$(tput setaf 6)"
  BOLD="$(tput bold)"; RESET="$(tput sgr0)"
else
  GREEN=""; YELLOW=""; RED=""; CYAN=""; BOLD=""; RESET=""
fi

say()  { printf "%s\n" "$*" | tee -a "$LOG" >/dev/null; }
die()  { finish_ui 2>/dev/null || :; printf "%sERROR:%s %s\n" "$RED" "$RESET" "$*" | tee -a "$LOG" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  if have sudo; then SUDO="sudo"
  elif have doas; then SUDO="doas"
  fi
fi

is_utf(){ echo "${LC_ALL:-${LANG:-}}" | grep -qi 'utf-8'; }
repeat(){ n=$1; ch=$2; i=0; out=""; while [ "$i" -lt "$n" ]; do out="$out$ch"; i=$((i+1)); done; printf "%s" "$out"; }
term_width(){ w=80; if command -v tput >/dev/null 2>&1; then w=$(tput cols 2>/dev/null || echo 80); fi; [ "$w" -gt 24 ] || w=80; echo "$w"; }

if is_utf; then FIL="█"; EMP="░"; else FIL="#"; EMP="-"; fi
TOTAL=13
STEP=0

draw_bar(){
  n=$1; tot=$2; msg=$3
  tw=$(term_width); bw=$(( tw - 32 ))
  [ "$bw" -lt 12 ] && bw=12
  [ "$bw" -gt 60 ] && bw=60
  [ "$tot" -gt 0 ] || tot=1
  pct=$(( n*100 / tot ))
  fill=$(( n*bw / tot ))
  empty=$(( bw - fill ))
  bar="$(repeat "$fill" "$FIL")$(repeat "$empty" "$EMP")"
  printf "\r\033[K%s%s[%s]%s %d/%d (%d%%) %s" "$CYAN" "$BOLD" "$bar" "$RESET" "$n" "$tot" "$pct" "$msg"
  printf "[%s] %d/%d (%d%%) %s\n" "$(repeat "$fill" "#")$(repeat "$empty" "-")" "$n" "$tot" "$pct" "$msg" >>"$LOG"
}
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
  while kill -0 "$pid" 2>/dev/null; do
    i=$(( (i+1) % 4 ))
    c=$(printf %s "$frames" | cut -c $((i+1)))
    printf "\r\033[K[%s] %s" "$c" "$msg"
    sleep 0.1
  done
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

auth_once(){
  [ -z "$SUDO" ] && return 0
  printf "\r\033[K%s%sElevating privileges (may prompt once)...%s\n" "$CYAN" "$BOLD" "$RESET"
  if [ "$SUDO" = "sudo" ]; then sudo -v
  else $SUDO true
  fi
}

run_root(){
  msg="$1"; cmd="$2"
  if [ -z "$SUDO" ]; then
    spinner "$msg" "$cmd"
    return
  fi
  if $SUDO -n true 2>/dev/null; then
    spinner "$msg" "$SUDO $cmd"
  else
    printf "\r\033[K%s%s%s %s\n" "$YELLOW" "[auth]" "$RESET" "$msg"
    $SUDO sh -c "$cmd" 2>&1 | tee -a "$LOG"
    draw_bar "$STEP" "$TOTAL" "$msg"
  fi
}

# detect pkg manager
PKG=""
if [ -r /etc/os-release ]; then . /etc/os-release || true; fi
case "${ID:-}" in
  alpine|postmarketos|chimera) PKG="apk" ;;
  arch|manjaro|endeavouros|arco|artix) PKG="pacman" ;;
  debian|ubuntu|pop|elementary|linuxmint|zorin) PKG="apt" ;;
  fedora|rhel|centos|rocky|almalinux) PKG="dnf" ;;
  opensuse*|sles) PKG="zypper" ;;
  gentoo) PKG="emerge" ;;
  void) PKG="xbps-install" ;;
  solus) PKG="eopkg" ;;
esac
[ -z "$PKG" ] && { have apk && PKG="apk" || :; }
[ -z "$PKG" ] && { have apt && PKG="apt" || :; }
[ -z "$PKG" ] && { have dnf && PKG="dnf" || :; }
[ -z "$PKG" ] && { have yum && PKG="yum" || :; }
[ -z "$PKG" ] && { have pacman && PKG="pacman" || :; }
[ -z "$PKG" ] && { have zypper && PKG="zypper" || :; }
[ -z "$PKG" ] && { have xbps-install && PKG="xbps-install" || :; }
[ -z "$PKG" ] && { have eopkg && PKG="eopkg" || :; }
[ -z "$PKG" ] && { have emerge && PKG="emerge" || :; }
[ -z "$PKG" ] && { have brew && [ "$(uname -s)" = "Darwin" ] && PKG="brew" || :; }

need_rust(){
  have cargo && have rustc && return 1 || return 0
}

install_rust(){
  case "$PKG" in
    apk)
      run_root "Installing rust + cargo..." "apk add --no-cache rust cargo"
      ;;
    apt)
      run_root "Updating APT index..." "sh -c 'apt-get update -y || apt update -y'"
      run_root "Installing rust + cargo..." "sh -c 'apt-get install -y rustc cargo || apt install -y rustc cargo'"
      ;;
    dnf|yum)
      run_root "Installing rust..." "$PKG install -y rust cargo || $PKG install -y rustc cargo || true"
      ;;
    pacman)
      run_root "Syncing pacman..." "pacman -Sy --noconfirm"
      run_root "Installing rust..." "pacman -S --needed --noconfirm rust"
      ;;
    zypper)
      run_root "Installing rust..." "zypper -n install rust cargo"
      ;;
    xbps-install)
      run_root "Installing rust..." "xbps-install -Sy rust cargo"
      ;;
    eopkg)
      run_root "Installing rust..." "eopkg -y it rust cargo || true"
      ;;
    emerge)
      run_root "Emerging rust..." "emerge --quiet-build=y --oneshot dev-lang/rust || true"
      ;;
    brew)
      spinner "Installing rust (brew)..." "brew install rust || true"
      ;;
    *)
      say "Cannot auto-install Rust for this OS. Install rustc + cargo manually."
      ;;
  esac
}

printf "%s%s>> Installing %s <<%s\n" "$CYAN" "$BOLD" "$APP_NAME" "$RESET" | tee -a "$LOG" >/dev/null

next "Checking environment"
next "Preparing privileges"
auth_once
draw_bar "$STEP" "$TOTAL" "Preparing privileges"

next "Detecting package manager"
[ -n "$PKG" ] || say "No supported package manager detected — will try to build anyway."

next "Checking Rust toolchain"
if need_rust; then
  install_rust
  have cargo || die "cargo is still missing after attempted install."
  have rustc || die "rustc is still missing after attempted install."
fi

next "Verifying cargo"
spinner "cargo fetch (warmup)..." "cargo fetch --locked 2>/dev/null || cargo fetch 2>/dev/null || true"

next "Building (release)"
spinner "cargo build --release ..." "cargo build --release"

TARGET_BIN="./target/release/$CARGO_BIN"
[ -x "$TARGET_BIN" ] || {
  # maybe someone switched the crate name
  ALT="./target/release/$BIN_NAME"
  [ -x "$ALT" ] || die "Build succeeded but neither $TARGET_BIN nor $ALT found."
  TARGET_BIN="$ALT"
}

TARGET_PREFIX="$DEFAULT_PREFIX"
INSTALL_CMD=""
if [ -n "$SUDO" ]; then
  INSTALL_CMD="install -m 755 '$TARGET_BIN' '$TARGET_PREFIX/bin/$BIN_NAME'"
else
  if [ "$(id -u)" -ne 0 ]; then
    TARGET_PREFIX="$USER_PREFIX"
    INSTALL_CMD="install -m 755 '$TARGET_BIN' '$TARGET_PREFIX/bin/$BIN_NAME'"
    printf "%sNote:%s installing to %s/bin (user)\n" "$YELLOW" "$RESET" "$TARGET_PREFIX" | tee -a "$LOG" >/dev/null
  else
    INSTALL_CMD="install -m 755 '$TARGET_BIN' '$TARGET_PREFIX/bin/$BIN_NAME'"
  fi
fi

next "Installing binary"
if [ -n "$SUDO" ]; then
  run_root "Installing..." "$INSTALL_CMD"
else
  spinner "Installing..." "$INSTALL_CMD"
fi

next "Installing man page (if present)"
if [ -f "./${BIN_NAME}.1" ]; then
  MAN_DIR="${TARGET_PREFIX}/share/man/man1"
  run_root "Copying man page..." "mkdir -p '$MAN_DIR' && install -m 0644 './${BIN_NAME}.1' '$MAN_DIR/${BIN_NAME}.1'"
  if have gzip; then run_root "Compressing man page..." "gzip -f -9 '$MAN_DIR/${BIN_NAME}.1' 2>/dev/null || true"; fi
  if have mandb; then run_root "Refreshing man database..." "mandb -q 2>/dev/null || true"
  elif have makewhatis; then run_root "Refreshing man database..." "makewhatis 2>/dev/null || true"; fi
else
  printf "No local man page; skipping.\n" >>"$LOG"
fi

next "Persisting repo path"
REPO_NOTE_DIR="${TARGET_PREFIX}/share/${APP_NAME}"
run_root "Saving repo location..." "mkdir -p '$REPO_NOTE_DIR' && printf %s \"$SCRIPT_DIR\" > '$REPO_NOTE_DIR/repo'"

next "Checking PATH"
BIN_DIR="${TARGET_PREFIX}/bin"
if ! printf "%s" ":$PATH:" | grep -q ":$BIN_DIR:"; then
  PROFILE=""
  [ -n "${ZDOTDIR-}" ] && [ -w "${ZDOTDIR}/.zprofile" 2>/dev/null ] && PROFILE="${ZDOTDIR}/.zprofile"
  [ -z "$PROFILE" ] && [ -w "$HOME/.zprofile" 2>/dev/null ] && PROFILE="$HOME/.zprofile"
  [ -z "$PROFILE" ] && [ -w "$HOME/.bash_profile" 2>/dev/null ] && PROFILE="$HOME/.bash_profile"
  [ -z "$PROFILE" ] && PROFILE="$HOME/.profile"
  mkdir -p "$(dirname "$PROFILE")" 2>/dev/null || :
  if ! grep -F "$BIN_DIR" "$PROFILE" >/dev/null 2>&1; then
    printf '\n# Added by %s installer\nexport PATH="%s:$PATH"\n' "$APP_NAME" "$BIN_DIR" >> "$PROFILE"
    say "Added ${BIN_DIR} to PATH in ${PROFILE}."
    say "Restart shell or run: ${BOLD}export PATH=\"${BIN_DIR}:\$PATH\"${RESET}"
  fi
fi

next "Done"
finish_ui
printf "%s%s✅ %s installed successfully.%s\n" "$GREEN" "$BOLD" "$APP_NAME" "$RESET"
printf "Log: %s\n" "$LOG"
