#!/bin/sh
# Install the latest Catabase release. Requires only macOS system utilities.
set -eu

fail() { printf 'Catabase: %s\n' "$*" >&2; exit 1; }

usage() {
  printf '%s\n' 'Usage: sh install.sh [--dir /absolute/install/directory]' \
    'Installs Catabase.app into ~/Applications by default. No sudo or developer tools required.'
}

main() {
  install_dir=${HOME:?HOME is not set}/Applications
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --dir) [ "$#" -ge 2 ] || fail '--dir requires an absolute directory.'; install_dir=$2; shift 2 ;;
      -h|--help) usage; return ;;
      *) usage >&2; fail "Unknown argument: $1" ;;
    esac
  done
  case "$install_dir" in /*) ;; *) fail 'The installation directory must be absolute.' ;; esac
  [ "$(uname -s)" = Darwin ] || fail 'This installer supports macOS only.'
  case "$(uname -m)" in arm64|x86_64) ;; *) fail 'Catabase requires Apple Silicon or Intel.' ;; esac
  for utility in curl ditto plutil shasum codesign pgrep unzip file awk mktemp; do
    command -v "$utility" >/dev/null 2>&1 || fail "Missing macOS utility: $utility"
  done
  [ "$(id -u)" -ne 0 ] || fail 'Run this installer as your normal user, without sudo.'

  ensure_closed() {
    if pgrep -x catabase >/dev/null 2>&1; then
      fail 'Quit Catabase before installing; your saved workspace will be preserved.'
    else
      process_status=$?
      [ "$process_status" -eq 1 ] || fail 'Could not check whether Catabase is running.'
    fi
  }
  ensure_closed

  release_base=https://github.com/smolijar/catabase/releases
  printf 'Finding the latest Catabase release…\n'
  release_url=$(curl --fail --silent --show-error --location --head \
    --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 60 --retry 2 \
    --output /dev/null --write-out '%{url_effective}' "$release_base/latest") \
    || fail 'No downloadable release was found. See https://github.com/smolijar/catabase/releases.'
  # Pin both downloads to this release; a newer publication cannot mix assets.
  case "$release_url" in "$release_base/tag/"*) release_tag=${release_url#"$release_base/tag/"} ;; *) fail 'Unexpected release URL.' ;; esac
  case "$release_tag" in ''|*[!v0-9.]*) fail 'Unexpected release version.' ;; esac
  printf '%s\n' "$release_tag" | awk '/^v[0-9]+\.[0-9]+\.[0-9]+$/ { valid=1 } END { exit !valid }' \
    || fail 'Unexpected release version.'

  mkdir -p "$install_dir" || fail "Cannot create $install_dir. Choose a writable directory with --dir."
  [ -w "$install_dir" ] || fail "Cannot write to $install_dir. Choose a writable directory with --dir."
  target=$install_dir/Catabase.app
  [ ! -L "$target" ] || fail 'The destination is a symbolic link; choose another installation directory.'
  if [ -e "$target" ]; then
    [ -d "$target" ] || fail 'The destination exists and is not an application directory.'
    [ "$(plutil -extract CFBundleIdentifier raw -o - "$target/Contents/Info.plist" 2>/dev/null)" = com.catabase.app ] \
      || fail 'The destination is not a Catabase application.'
  fi
  lock=$install_dir/.catabase-install.lock
  mkdir "$lock" 2>/dev/null || fail "Another installer may be running. If none is running, remove $lock and retry."
  stage=$(mktemp -d "$install_dir/.catabase-install.XXXXXXXX") || {
    rmdir "$lock"
    fail 'Cannot create a staging directory.'
  }
  installed=false
  cleanup() {
    cleanup_status=$?
    trap - EXIT HUP INT TERM
    if [ "$installed" = false ] && [ -d "$stage/previous.app" ]; then
      if [ ! -e "$target" ] && mv "$stage/previous.app" "$target"; then
        printf 'Restored the previous Catabase installation.\n' >&2
      else
        printf 'Previous installation retained at %s/previous.app\n' "$stage" >&2
        exit 1
      fi
    fi
    rm -rf "$stage"
    rmdir "$lock"
    exit "$cleanup_status"
  }
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' HUP TERM

  archive=Catabase-macos-universal.app.zip
  printf 'Downloading Catabase %s…\n' "$release_tag"
  for asset in "$archive" SHA256SUMS; do
    curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
      --connect-timeout 15 --max-time 600 --retry 2 \
      --output "$stage/$asset" "$release_base/download/$release_tag/$asset" \
      || fail "Could not download $asset. The existing installation has not changed."
  done
  expected=$(awk -v name="$archive" '
    $2 == name || $2 == "./" name || $2 == "*" name || $2 == "*./" name {
      if (length($1) != 64 || $1 ~ /[^0-9a-fA-F]/) exit 1
      hash=$1; count++
    }
    END { if (count != 1) exit 1; print tolower(hash) }
  ' "$stage/SHA256SUMS") || fail 'The release checksum for the app is missing or invalid.'
  actual=$(shasum -a 256 "$stage/$archive" | awk '{ print $1 }')
  [ "$actual" = "$expected" ] || fail 'Download checksum mismatch. Nothing was installed.'

  unzip -Z1 "$stage/$archive" > "$stage/archive-files" || fail 'Cannot read the application archive.'
  awk '
    /^\// || /(^|\/)\.\.(\/|$)/ || /\\/ { exit 1 }
    !/^Catabase\.app(\/|$)/ && !/^__MACOSX(\/|$)/ { exit 1 }
    END { if (NR == 0) exit 1 }
  ' "$stage/archive-files" || fail 'The archive contains unexpected paths.'
  mkdir "$stage/unpacked"
  ditto -x -k "$stage/$archive" "$stage/unpacked" || fail 'Could not unpack the application.'
  candidate=$stage/unpacked/Catabase.app
  [ -d "$candidate" ] && [ ! -L "$candidate" ] || fail 'The archive does not contain Catabase.app.'
  plist=$candidate/Contents/Info.plist
  [ "$(plutil -extract CFBundleIdentifier raw -o - "$plist")" = com.catabase.app ] || fail 'Unexpected application identifier.'
  [ "$(plutil -extract CFBundleExecutable raw -o - "$plist")" = catabase ] || fail 'Unexpected application executable.'
  [ "$(plutil -extract CFBundleShortVersionString raw -o - "$plist")" = "${release_tag#v}" ] || fail 'The app version does not match the release.'
  executable=$candidate/Contents/MacOS/catabase
  [ -f "$executable" ] && [ -x "$executable" ] && [ ! -L "$executable" ] || fail 'The application executable is missing.'
  # `lipo` needs developer tools on some Macs; `file` ships with the OS.
  file -b "$executable" | awk '/Mach-O universal binary/ && /arm64/ && /x86_64/ { valid=1 } END { exit !valid }' \
    || fail 'The app does not contain both Apple Silicon and Intel binaries.'
  codesign --verify --deep --strict "$candidate" || fail 'Application signature verification failed.'

  ensure_closed
  if [ -e "$target" ]; then mv "$target" "$stage/previous.app" || fail 'Could not move the previous installation.'; fi
  mv "$candidate" "$target" || fail 'Could not install the new app.'
  installed=true
  printf '\nInstalled Catabase %s at %s\n' "${release_tag#v}" "$target"
  printf '%s\n' 'Open Catabase from that folder. Saved connections, tabs and history are kept.' \
    'Releases without Apple notarization may be blocked by macOS. This installer does not change Gatekeeper settings.'
}

main "$@"
