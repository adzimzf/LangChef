#!/usr/bin/env bash
# Install or update LangChef from its GitHub Releases.
#
#   curl -fsSL https://raw.githubusercontent.com/adzimzf/LangChef/main/scripts/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/adzimzf/LangChef/main/scripts/install.sh | bash -s -- --android
#
# Running it again updates LangChef when a newer release is out. Your data
# (~/.app-data/langchef on desktop, the app's storage on Android) is kept.
#
# Options:
#   --android          install the APK on a phone connected with adb (USB debugging on);
#                      without adb, the APK is saved to ~/Downloads instead
#   --version vX.Y.Z   install this release instead of the latest one
#   --dir PATH         macOS: folder for LangChef.app (default /Applications,
#                      or ~/Applications when /Applications isn't writable)
#   --check            only say whether an update is available (exit 0 = up to date, 10 = update)
#   --force            reinstall even when this version is already installed (allows downgrades)
#   -h, --help         show this help
#
# Set GITHUB_TOKEN to avoid GitHub's API rate limit for anonymous requests.

set -euo pipefail

REPO="adzimzf/LangChef"
APP_NAME="LangChef"
ANDROID_PACKAGE="app.langchef"

target=""
want_version=""
install_dir=""
check_only=0
force=0

say()  { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
usage() { sed -n '2,/^$/{s/^# \{0,1\}//;p;}' "$0" 2>/dev/null || true; }

while [ $# -gt 0 ]; do
  case "$1" in
    --android) target="android" ;;
    --version) [ $# -ge 2 ] || die "--version needs a value"; want_version="$2"; shift ;;
    --version=*) want_version="${1#*=}" ;;
    --dir) [ $# -ge 2 ] || die "--dir needs a value"; install_dir="$2"; shift ;;
    --dir=*) install_dir="${1#*=}" ;;
    --check) check_only=1 ;;
    --force) force=1 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
  shift
done

command -v curl >/dev/null || die "curl is required"

if [ -z "$target" ]; then
  case "$(uname -s)" in
    Darwin) target="macos" ;;
    *) die "LangChef releases are built for macOS and Android only. On this system use --android to install on a connected phone." ;;
  esac
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/langchef-install.XXXXXX")"
mount_point=""
cleanup() {
  if [ -n "$mount_point" ]; then hdiutil detach "$mount_point" -quiet >/dev/null 2>&1 || true; fi
  rm -rf "$tmp"
}
trap cleanup EXIT

# ---- release lookup --------------------------------------------------------

api() {
  local args=(-fsSL -H "Accept: application/vnd.github+json")
  [ -n "${GITHUB_TOKEN:-}" ] && args+=(-H "Authorization: Bearer $GITHUB_TOKEN")
  curl "${args[@]}" "https://api.github.com/repos/$REPO/$1"
}

if [ -n "$want_version" ]; then
  tag="v${want_version#v}"
  release_json="$(api "releases/tags/$tag")" || die "release $tag not found"
else
  release_json="$(api "releases/latest")" || die "could not read the latest release (GitHub rate limit? set GITHUB_TOKEN)"
  tag="$(printf '%s\n' "$release_json" | sed -n 's/^  "tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$tag" ] || die "could not find the latest release tag"
fi
version="${tag#v}"

case "$target" in
  macos)
    case "$(uname -m)" in
      arm64|aarch64) arch="aarch64" ;;
      x86_64) arch="x64" ;;
      *) die "unsupported Mac architecture: $(uname -m)" ;;
    esac
    asset="${APP_NAME}_${version}_macos_${arch}.dmg" ;;
  android)
    asset="${APP_NAME}_${version}_android_arm64.apk" ;;
esac

# The asset's sha256 from the release ("digest": "sha256:..."), empty if GitHub didn't record one.
asset_sha256() {
  printf '%s\n' "$release_json" | awk -v want="\"name\": \"$1\"" '
    index($0, want) { found = 1; next }
    found && /"digest":/ { if (match($0, /sha256:[0-9a-f]+/)) print substr($0, RSTART + 7, RLENGTH - 7); exit }
    found && /"browser_download_url":/ { exit }
  '
}

printf '%s\n' "$release_json" | grep -q "\"name\": \"$asset\"" \
  || die "release $tag has no $asset"

# ---- installed version -----------------------------------------------------

installed=""
adb_ok=0
if [ "$target" = "macos" ]; then
  if [ -z "$install_dir" ]; then
    if [ -d "/Applications/$APP_NAME.app" ] || [ -w /Applications ]; then
      install_dir="/Applications"
    else
      install_dir="$HOME/Applications"
    fi
    # Update an existing copy in ~/Applications rather than adding a second one.
    if [ ! -d "/Applications/$APP_NAME.app" ] && [ -d "$HOME/Applications/$APP_NAME.app" ]; then
      install_dir="$HOME/Applications"
    fi
  fi
  app_path="$install_dir/$APP_NAME.app"
  if [ -f "$app_path/Contents/Info.plist" ]; then
    installed="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app_path/Contents/Info.plist" 2>/dev/null || true)"
  fi
else
  if command -v adb >/dev/null && [ "$(adb devices | awk 'NR > 1 && $2 == "device"' | wc -l)" -ge 1 ]; then
    adb_ok=1
    installed="$(adb shell dumpsys package "$ANDROID_PACKAGE" 2>/dev/null | sed -n 's/.*versionName=\([^[:space:]]*\).*/\1/p' | head -n1 | tr -d '\r')"
  fi
fi

if [ -n "$installed" ]; then
  say "Installed: $APP_NAME $installed — release: $version"
else
  say "Release: $APP_NAME $version (not installed yet)"
fi

if [ "$check_only" -eq 1 ]; then
  if [ "$installed" = "$version" ]; then echo "Up to date."; exit 0; fi
  echo "Update available: $version"; exit 10
fi

if [ "$installed" = "$version" ] && [ "$force" -eq 0 ]; then
  say "Already up to date."
  exit 0
fi

# ---- download --------------------------------------------------------------

file="$tmp/$asset"
say "Downloading $asset"
curl -fL --progress-bar -o "$file" "https://github.com/$REPO/releases/download/$tag/$asset" \
  || die "download failed"

expected="$(asset_sha256 "$asset")"
if [ -n "$expected" ]; then
  if command -v shasum >/dev/null; then
    actual="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    actual="$(sha256sum "$file" | awk '{print $1}')"
  fi
  [ "$actual" = "$expected" ] || die "checksum mismatch for $asset (expected $expected, got $actual)"
  say "Checksum OK"
else
  warn "the release lists no checksum for $asset; skipping verification"
fi

# ---- install ---------------------------------------------------------------

if [ "$target" = "macos" ]; then
  mount_point="$tmp/mnt"
  mkdir -p "$mount_point"
  hdiutil attach "$file" -nobrowse -readonly -quiet -mountpoint "$mount_point" || die "could not open the DMG"
  [ -d "$mount_point/$APP_NAME.app" ] || die "$APP_NAME.app not found in the DMG"

  if pgrep -xq "$APP_NAME"; then
    say "Quitting $APP_NAME"
    osascript -e "tell application \"$APP_NAME\" to quit" >/dev/null 2>&1 || true
    for _ in $(seq 1 20); do pgrep -xq "$APP_NAME" || break; sleep 0.5; done
    pgrep -xq "$APP_NAME" && die "$APP_NAME is still running; quit it and run this again"
  fi

  # macOS ships bash 3.2, where an empty "${array[@]}" trips set -u, hence a flag and a helper.
  use_sudo=0
  mkdir -p "$install_dir" 2>/dev/null || true
  if [ ! -w "$install_dir" ]; then
    say "$install_dir needs administrator rights"
    use_sudo=1
  fi
  run() { if [ "$use_sudo" -eq 1 ]; then sudo "$@"; else "$@"; fi; }

  say "Installing to $app_path"
  staged="$install_dir/.$APP_NAME.app.new"
  run rm -rf "$staged"
  run ditto "$mount_point/$APP_NAME.app" "$staged"
  run rm -rf "$app_path"
  run mv "$staged" "$app_path"
  # The build isn't notarized: drop the download quarantine (and any other extended
  # attributes, which codesign refuses to sign over) so Gatekeeper lets it open.
  run xattr -cr "$app_path" 2>/dev/null || true
  # The bundle isn't signed either; Apple Silicon can call such an app "damaged".
  # An ad-hoc signature (no developer identity) makes it valid on this Mac.
  if command -v codesign >/dev/null; then
    say "Signing $APP_NAME.app for this Mac (ad-hoc)"
    run codesign --force --deep --sign - "$app_path" >/dev/null 2>&1 \
      || warn "ad-hoc signing failed; if macOS says the app is damaged, run: codesign --force --deep --sign - \"$app_path\""
  fi

  say "$APP_NAME $version installed. Open it from $install_dir or Launchpad."
else
  if [ "$adb_ok" -eq 1 ]; then
    say "Installing on the connected phone"
    flags=(-r)
    [ "$force" -eq 1 ] && flags+=(-d)
    if ! out="$(adb install "${flags[@]}" "$file" 2>&1)"; then
      printf '%s\n' "$out" >&2
      if printf '%s' "$out" | grep -q INSTALL_FAILED_UPDATE_INCOMPATIBLE; then
        die "the installed app was signed with another key. Uninstall it first (this deletes its data — sync or back up first): adb uninstall $ANDROID_PACKAGE"
      fi
      die "adb install failed"
    fi
    say "$APP_NAME $version installed on the phone."
  else
    dest="$HOME/Downloads"
    mkdir -p "$dest"
    cp "$file" "$dest/$asset"
    warn "no phone found over adb (install Android platform-tools and turn on USB debugging to install directly)"
    say "Saved $dest/$asset — copy it to your phone and open it to install or update."
  fi
fi
