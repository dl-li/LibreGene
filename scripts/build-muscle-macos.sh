#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d /tmp/libregene-muscle-build.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

for command in curl tar make uv; do
  command -v "$command" >/dev/null
 done

if command -v g++-16 >/dev/null; then
  CXX=g++-16
  CC=gcc-16
else
  echo "GCC 16 is required (brew install gcc)" >&2
  exit 1
fi

GCC_PREFIX=$(brew --prefix gcc)
LIBGOMP="$GCC_PREFIX/lib/gcc/16/libgomp.a"
if [[ ! -f "$LIBGOMP" ]]; then
  echo "Missing $LIBGOMP" >&2
  exit 1
fi

curl --http1.1 -fL --retry 4 --retry-all-errors \
  https://codeload.github.com/rcedgar/muscle/tar.gz/refs/tags/v5.3 \
  -o "$WORK/muscle.tar.gz"
mkdir "$WORK/source"
tar -xzf "$WORK/muscle.tar.gz" -C "$WORK/source" --strip-components=1
curl --http1.1 -fL --retry 4 --retry-all-errors \
  https://raw.githubusercontent.com/rcedgar/vcxproj_make/806d016/vcxproj_make.py \
  -o "$WORK/source/src/vcxproj_make.py"

cd "$WORK/source/src"
uv run python vcxproj_make.py --cppcompiler "$CXX" --ccompiler "$CC" --std c++17 --pthread
make -j"$(sysctl -n hw.ncpu)" ../bin/muscle \
  LDFLAGS="-ffast-math -O3 -static-libstdc++ -static-libgcc $LIBGOMP -lpthread"

case "$(uname -m)" in
  arm64) TARGET=aarch64-apple-darwin ;;
  x86_64) TARGET=x86_64-apple-darwin ;;
  *) echo "Unsupported macOS architecture: $(uname -m)" >&2; exit 1 ;;
esac

cp ../bin/muscle "$ROOT/src-tauri/binaries/muscle-$TARGET"
chmod +x "$ROOT/src-tauri/binaries/muscle-$TARGET"
"$ROOT/src-tauri/binaries/muscle-$TARGET" -version
otool -L "$ROOT/src-tauri/binaries/muscle-$TARGET"
