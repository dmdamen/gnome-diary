#!/usr/bin/env bash
# Development helper. Rust, GTK and every other tool run in a podman
# container, so nothing needs installing on this machine.
#
#   ./dev.sh image        build the development image (localhost/gnome-diary-dev)
#   ./dev.sh test         fmt + clippy + all tests with the 95% coverage gate
#                         (HTML report in target/llvm-cov/html)
#   ./dev.sh quick [args] cargo test without coverage, for fast red/green loops
#   ./dev.sh run          start the app on this Wayland session
#                         (throwaway home in ~/.cache/gnome-diary-dev/home)
#   ./dev.sh cargo ...    any cargo command inside the container
#   ./dev.sh shell        interactive shell in the container
#   ./dev.sh flatpak      build build/diary.flatpak
#   ./dev.sh install      install build/diary.flatpak for this user (touches the host)
set -euo pipefail
cd "$(dirname "$0")"

IMAGE=${IMAGE:-localhost/gnome-diary-dev}
APP_ID=io.github.diary.Diary
COVERAGE_MIN=${COVERAGE_MIN:-95}

# in_container [podman options --] command...
in_container() {
  local opts=()
  while [[ $# -gt 0 && $1 != -- ]]; do opts+=("$1"); shift; done
  [[ ${1:-} == -- ]] && shift
  podman image exists "$IMAGE" || podman build -t "$IMAGE" -f container/Containerfile container
  podman run --rm --userns=keep-id --security-opt label=disable \
    -v "$PWD":/src -v gnome-diary-cargo:/cargo -w /src \
    -e CARGO_TERM_COLOR=always "${opts[@]}" "$IMAGE" "$@"
}

# Commands run with a headless Wayland compositor, for the GTK tests.
HEADLESS='
  export XDG_RUNTIME_DIR=$(mktemp -d) WAYLAND_DISPLAY=wayland-test GDK_BACKEND=wayland
  export GSETTINGS_BACKEND=memory GTK_A11Y=none NO_AT_BRIDGE=1 GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1
  weston --backend=headless --socket=wayland-test --idle-time=0 >/tmp/weston.log 2>&1 &
  for _ in $(seq 50); do [[ -S $XDG_RUNTIME_DIR/wayland-test ]] && break; sleep 0.1; done
'

case ${1:-} in
  image)
    podman build -t "$IMAGE" -f container/Containerfile container
    ;;
  test)
    in_container -- bash -euc "$HEADLESS
      cargo fmt --all --check
      cargo clippy --workspace --all-targets -- -D warnings
      cargo llvm-cov --workspace --html --ignore-filename-regex 'crates/diary/src/main.rs'
      cargo llvm-cov report --summary-only --ignore-filename-regex 'crates/diary/src/main.rs' \
        --fail-under-lines $COVERAGE_MIN"
    ;;
  quick)
    shift
    in_container -- bash -euc "$HEADLESS cargo test --workspace \"\$@\"" bash "$@"
    ;;
  cargo)
    shift
    in_container -- cargo "$@"
    ;;
  shell)
    in_container -it -- bash
    ;;
  run)
    home=$HOME/.cache/gnome-diary-dev/home
    mkdir -p "$home"
    in_container -it -v "$home":/home/dev -e HOME=/home/dev \
      -v "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY":/tmp/xdg/"$WAYLAND_DISPLAY" \
      -e XDG_RUNTIME_DIR=/tmp/xdg -e WAYLAND_DISPLAY="$WAYLAND_DISPLAY" -e GDK_BACKEND=wayland \
      -e GSETTINGS_BACKEND=keyfile -- cargo run -p diary
    ;;
  flatpak)
    mkdir -p build
    in_container -- bash -euc 'rm -rf build/vendor && cargo vendor --locked build/vendor >build/cargo-vendor-config.toml'
    podman run --rm --privileged -v "$PWD":/src:z -v gnome-diary-flatpak:/root/.local/share/flatpak -w /src \
      registry.fedoraproject.org/fedora:45 bash -euc "
        dnf -y -q install flatpak flatpak-builder >/dev/null
        flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
        flatpak-builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
          --repo=build/repo build/app build-aux/$APP_ID.json
        flatpak build-bundle build/repo build/diary.flatpak $APP_ID"
    ls -l build/diary.flatpak
    ;;
  install)
    flatpak install --user -y build/diary.flatpak
    ;;
  *)
    sed -n '2,15p' "$0"
    exit 1
    ;;
esac
