#!/usr/bin/env bash
# Bootstrap dotfix on a fresh Mac.
#
#   /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/NoiXdev/dotfix/v0.1.0/install.sh)"
#
# Always fetch a release tag, never `main`: an intermediate commit must never
# be able to break a machine that is being set up.
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "dotfix is macOS only" >&2
  exit 1
fi

if ! command -v brew >/dev/null 2>&1; then
  echo "==> installing Homebrew"
  /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
  if [[ -x /opt/homebrew/bin/brew ]]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
  fi
fi

echo "==> installing dotfix"
brew tap NoiXdev/tap
brew install dotfix

cat <<'EOF'

dotfix is installed. Next:

  dotfix init                  # clone an existing data repository
  dotfix init --set-up-new     # create one from this machine

EOF
