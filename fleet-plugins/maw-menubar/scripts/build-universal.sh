#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

swift build -c release --arch arm64 --scratch-path .build/arm64
swift build -c release --arch x86_64 --scratch-path .build/x86_64

mkdir -p bin
/usr/bin/lipo -create \
  .build/arm64/arm64-apple-macosx/release/maw-menubar \
  .build/x86_64/x86_64-apple-macosx/release/maw-menubar \
  -output bin/maw-menubar
chmod 755 bin/maw-menubar
/usr/bin/codesign --force --sign - bin/maw-menubar

archs=$(/usr/bin/lipo -archs bin/maw-menubar)
case " $archs " in
  *" arm64 "*) ;;
  *) echo "maw-menubar: universal helper is missing arm64" >&2; exit 1 ;;
esac
case " $archs " in
  *" x86_64 "*) ;;
  *) echo "maw-menubar: universal helper is missing x86_64" >&2; exit 1 ;;
esac

shasum -a 256 bin/maw-menubar
