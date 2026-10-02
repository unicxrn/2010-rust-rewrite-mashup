#!/usr/bin/env bash
# Builds 2010-Rust-Rewrite-Mashup-x86_64.AppImage into dist/.
# Needs: rustup toolchain, gcc, cmake, git, python3 (with venv), curl.
#
# Set APPIMAGE_UPDATE_INFO to embed update information in the AppImage, so
# tools like AppImageUpdate can find new releases (appimagetool also
# generates a .zsync file alongside dist/*.AppImage when zsyncmake is on
# PATH). Example, for GitHub releases:
#   APPIMAGE_UPDATE_INFO='gh-releases-zsync|owner|repo|latest|2010-Rust-Rewrite-Mashup-x86_64.AppImage.zsync'
# Left unset (the default), no update information is embedded.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="$ROOT/target/appimage"
APPDIR="$WORK/AppDir"
DOCDIR="$APPDIR/usr/share/doc/2010-rust-rewrite-mashup"
SKATE_REV=cb79689
EXTRACT_XISO_REV=3f5b62cfe68f000b0e3c8a30104973f3a297948e
# Latest tagged appimagetool release (checked against GitHub's release API);
# pinned instead of "continuous" so the tool itself doesn't move under us.
APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
mkdir -p "$WORK" "$ROOT/dist"

# PyInstaller 6.19 / numpy 2.2.6 (pinned in the skate engine's
# tools/requirements-setup.txt) don't support python3.14+; pick the newest
# supported interpreter available on PATH.
PYTHON=""
for cand in python3.13 python3.12 python3.11; do
  if command -v "$cand" >/dev/null 2>&1; then
    PYTHON="$cand"
    break
  fi
done
if [ -z "$PYTHON" ]; then
  echo "error: need one of python3.13, python3.12, python3.11 on PATH (numpy/PyInstaller pins don't support newer pythons)" >&2
  exit 1
fi
echo "== using $PYTHON for skate-convert venv"

echo "== game and launcher"
cargo build --manifest-path "$ROOT/Cargo.toml" --release -p launcher -p mashup_launcher

echo "== extract-xiso"
NEED_XISO_BUILD=0
if [ ! -x "$WORK/extract-xiso/build/extract-xiso" ]; then
  NEED_XISO_BUILD=1
elif [ "$(git -C "$WORK/extract-xiso" rev-parse HEAD 2>/dev/null || true)" != "$EXTRACT_XISO_REV" ]; then
  NEED_XISO_BUILD=1
fi
if [ "$NEED_XISO_BUILD" = 1 ]; then
  rm -rf "$WORK/extract-xiso"
  git clone https://github.com/XboxDev/extract-xiso "$WORK/extract-xiso"
  git -C "$WORK/extract-xiso" checkout -q "$EXTRACT_XISO_REV"
  cmake -S "$WORK/extract-xiso" -B "$WORK/extract-xiso/build" -DCMAKE_BUILD_TYPE=Release
  cmake --build "$WORK/extract-xiso/build" -j
fi

echo "== skate converter"
CONVERTER_PY="$ROOT/skate/converter/iw4l_skate_convert.py"
CONVERTER_STAMP="$WORK/skate-convert/.stamp"
EXPECTED_STAMP="$SKATE_REV $(sha256sum "$CONVERTER_PY" | awk '{print $1}')"
NEED_CONVERTER_BUILD=0
if [ ! -x "$WORK/skate-convert/skate-convert" ]; then
  NEED_CONVERTER_BUILD=1
elif [ ! -f "$CONVERTER_STAMP" ] || [ "$(cat "$CONVERTER_STAMP")" != "$EXPECTED_STAMP" ]; then
  NEED_CONVERTER_BUILD=1
fi
if [ "$NEED_CONVERTER_BUILD" = 1 ]; then
  [ -d "$WORK/skate-engine" ] || git clone https://github.com/SK8-ENGINE/skate-3-rust-engine "$WORK/skate-engine"
  git -C "$WORK/skate-engine" checkout -q "$SKATE_REV"
  STAGE="$WORK/skate-stage"
  rm -rf "$STAGE" && mkdir -p "$STAGE/tools"
  (cd "$WORK/skate-engine/tools" && find . -type f \( -name '*.py' -o -name '*.json' -o -name '*.txt' -o -name '*.md' -o -name '*.toml' -o -name LICENSE \) \
     ! -path '*/__pycache__/*' ! -name 'test_*.py' ! -path './mixamo_to_skate/*.json' ! -path '*blender*' \
     -exec install -D {} "$STAGE/tools/{}" \;)
  # Native RefPack backend; the converter loads it by this exact file name.
  rustc --edition 2024 --crate-type cdylib -C opt-level=3 -C panic=abort \
    "$WORK/skate-engine/tools/asset_pipeline/refpack_native.rs" -o "$STAGE/refpack.dll"
  "$PYTHON" -m venv "$WORK/venv"
  "$WORK/venv/bin/pip" install -q -r "$WORK/skate-engine/tools/requirements-setup.txt"
  rm -rf "$WORK/skate-convert"
  "$WORK/venv/bin/python" -m PyInstaller --noconfirm --clean --onefile --console --name skate-convert \
    --paths "$WORK/skate-engine" \
    --hidden-import numpy --hidden-import PIL.Image \
    --add-binary "$STAGE/refpack.dll:tools/asset_pipeline" \
    --add-data "$STAGE/tools:tools" \
    --exclude-module bpy --exclude-module mathutils --exclude-module tkinter \
    --exclude-module readline --exclude-module curses --exclude-module _curses --exclude-module _curses_panel \
    --exclude-module ssl --exclude-module _ssl --exclude-module _hashlib \
    --copy-metadata numpy --copy-metadata Pillow \
    --distpath "$WORK/skate-convert" --workpath "$WORK/pyi-build" --specpath "$WORK" \
    "$CONVERTER_PY"
  echo "$EXPECTED_STAMP" > "$CONVERTER_STAMP"
fi

echo "== AppDir"
rm -rf "$APPDIR"
install -Dm755 "$ROOT/target/release/iw4l"            "$APPDIR/usr/bin/iw4l"
install -Dm755 "$ROOT/target/release/mashup-launcher" "$APPDIR/usr/bin/mashup-launcher"
install -Dm755 "$WORK/skate-convert/skate-convert"    "$APPDIR/usr/bin/skate-convert"
install -Dm755 "$WORK/extract-xiso/build/extract-xiso" "$APPDIR/usr/bin/extract-xiso"
install -Dm755 "$ROOT/packaging/linux/AppRun"         "$APPDIR/AppRun"
install -Dm644 "$ROOT/packaging/linux/2010-rust-rewrite-mashup.desktop" "$APPDIR/2010-rust-rewrite-mashup.desktop"
install -Dm644 "$ROOT/packaging/linux/io.github.chasmlol.RustRewriteMashup.metainfo.xml" \
  "$APPDIR/usr/share/metainfo/io.github.chasmlol.RustRewriteMashup.metainfo.xml"
install -Dm644 "$ROOT/packaging/linux/icon-256.png"   "$APPDIR/2010-rust-rewrite-mashup.png"
install -Dm644 "$ROOT/packaging/linux/icon-256.png"   "$APPDIR/usr/share/icons/hicolor/256x256/apps/2010-rust-rewrite-mashup.png"
for f in LICENSE NOTICE crates/ui/assets/OFL-Oxanium.txt crates/console/assets/COPYING-FreeFont.txt; do
  install -Dm644 "$ROOT/$f" "$DOCDIR/$(basename "$f")"
done

echo "== third-party licences"
# extract-xiso: modified BSD, requires reproducing its notice with the binary.
install -Dm644 "$WORK/extract-xiso/LICENSE.TXT" "$DOCDIR/extract-xiso-LICENSE.txt"

# Fonts eframe compiles into mashup-launcher (its default_fonts feature).
EGUI_FONTS="$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --format-version 1 --locked \
  --filter-platform x86_64-unknown-linux-gnu \
  | "$PYTHON" -c 'import json,os,sys;print(next(os.path.dirname(p["manifest_path"]) for p in json.load(sys.stdin)["packages"] if p["name"]=="epaint_default_fonts"))')/fonts"
for pair in Hack-Regular.txt:Hack-LICENSE.txt UFL.txt:Ubuntu-Light-UFL.txt \
            OFL.txt:NotoEmoji-OFL.txt emoji-icon-font-mit-license.txt:emoji-icon-font-MIT.txt; do
  install -Dm644 "$EGUI_FONTS/${pair%%:*}" "$DOCDIR/licenses/egui-fonts/${pair#*:}"
done

# Everything the PyInstaller-bundled skate-convert carries: CPython, NumPy,
# Pillow, and the skate engine's own tools/ notices and vendored licences.
PY_SITE_PKGS="$WORK/venv/lib/$("$PYTHON" -c 'import sys;print("python%d.%d"%sys.version_info[:2])')/site-packages"

PY_LICENSE="$("$PYTHON" -c 'import sys,os;print(os.path.join(sys.base_prefix,"lib","python%d.%d"%sys.version_info[:2],"LICENSE.txt"))')"
if [ ! -f "$PY_LICENSE" ]; then
  PY_LICENSE="$(ls /usr/share/licenses/python*/LICENSE 2>/dev/null | head -1 || true)"
fi
if [ -z "${PY_LICENSE:-}" ] || [ ! -f "$PY_LICENSE" ]; then
  echo "error: could not find the Python interpreter's LICENSE (checked sys.base_prefix's stdlib dir and /usr/share/licenses/python*/LICENSE)" >&2
  exit 1
fi
install -Dm644 "$PY_LICENSE" "$DOCDIR/licenses/Python.txt"

NUMPY_DIST_INFO="$(find "$PY_SITE_PKGS" -maxdepth 1 -iname 'numpy-*.dist-info' | head -1)"
install -Dm644 "$NUMPY_DIST_INFO/LICENSE.txt" "$DOCDIR/licenses/NumPy.txt"
# Some numpy releases also ship a separate file for bundled third-party
# licences (e.g. vendored BLAS/LAPACK notices); include it when present.
if [ -f "$PY_SITE_PKGS/numpy/LICENSES_bundled.txt" ]; then
  install -Dm644 "$PY_SITE_PKGS/numpy/LICENSES_bundled.txt" "$DOCDIR/licenses/NumPy-bundled-third-party.txt"
fi

PILLOW_DIST_INFO="$(find "$PY_SITE_PKGS" -maxdepth 1 -iname 'pillow-*.dist-info' | head -1)"
install -Dm644 "$PILLOW_DIST_INFO/licenses/LICENSE" "$DOCDIR/licenses/Pillow.txt"

install -Dm644 "$WORK/skate-engine/docs/THIRD_PARTY_NOTICES.md" "$DOCDIR/licenses/skate-engine-THIRD_PARTY_NOTICES.md"

# tools/vendor/*/LICENSE* and any tools/**/licenses/* or tools/**/LICENSE*
# file, named after their path so provenance stays traceable.
while IFS= read -r -d '' f; do
  rel="${f#"$WORK"/skate-engine/tools/}"
  rel="${rel#vendor/}"
  name="$(echo "$rel" | tr '/' '-')"
  install -Dm644 "$f" "$DOCDIR/licenses/skate-engine-tools-$name"
done < <(find "$WORK/skate-engine/tools" -type f \( -iname 'LICENSE*' -o -path '*/licenses/*' \) -print0)

# Host system libraries PyInstaller pulled into the onefile bundle by
# scanning actual link dependencies (readline/curses/ssl were dropped above
# instead of licensed, since they're unused by this converter). Libraries
# vendored inside the numpy/Pillow wheels themselves (pillow.libs/,
# numpy.libs/, and their flattened top-level copies, identifiable by
# PyInstaller's "-<8 hex chars>" disambiguation suffix) are already covered
# by NumPy.txt/Pillow.txt and skipped here; so are the GCC runtime and
# libpython itself (covered by Python.txt).
echo "== system library licences for bundled host libs"
SYSLIB_DIR="$DOCDIR/licenses/system-libs"
BUNDLED_SOS="$("$WORK/venv/bin/pyi-archive_viewer" --recursive --brief "$WORK/skate-convert/skate-convert" 2>/dev/null \
  | sed 's/^ //' | grep -v '/' | grep '\.so')"
while IFS= read -r entry; do
  [ -n "$entry" ] || continue
  # Wheel-vendored copies carry PyInstaller's disambiguation hash; skip them.
  if echo "$entry" | grep -Eq -- '-[0-9a-f]{8}(\.so|-)'; then
    continue
  fi
  base="${entry%%.so*}"
  case "$base" in
    libgcc_s|libstdc++|libgomp|libquadmath|libgfortran|libpython3*) continue ;;
  esac
  LIBPATH="$(find /usr/lib /usr/lib64 /lib /lib64 -maxdepth 1 -name "${base}.so*" 2>/dev/null | head -1)"
  if [ -z "$LIBPATH" ]; then
    echo "WARNING: could not resolve a host path for bundled lib '$entry'; no licence copied" >&2
    continue
  fi
  PKG=""
  LICDIR=""
  if command -v pacman >/dev/null 2>&1; then
    PKG="$(pacman -Qoq "$LIBPATH" 2>/dev/null | head -1)"
    [ -n "$PKG" ] && LICDIR="/usr/share/licenses/$PKG"
  elif command -v dpkg >/dev/null 2>&1; then
    PKG="$(dpkg -S "$LIBPATH" 2>/dev/null | head -1 | cut -d: -f1)"
    [ -n "$PKG" ] && LICDIR="/usr/share/doc/$PKG"
  fi
  if [ -z "$PKG" ] || [ -z "$LICDIR" ] || [ ! -d "$LICDIR" ]; then
    echo "WARNING: no package or licence directory found for bundled lib '$entry' ($LIBPATH); no licence copied" >&2
    continue
  fi
  COPIED=0
  for lf in "$LICDIR"/*; do
    [ -f "$lf" ] || continue
    install -Dm644 "$lf" "$SYSLIB_DIR/${PKG}-$(basename "$lf")"
    COPIED=1
  done
  if [ "$COPIED" = 0 ]; then
    echo "WARNING: package '$PKG' (for bundled lib '$entry') has no files under $LICDIR; no licence copied" >&2
  fi
done <<< "$BUNDLED_SOS"

# Minimum runtime requirements, since there's no desktop-file field for it.
cat > "$DOCDIR/README-linux.txt" <<'EOF'
2010 Rust Rewrite Mashup: Linux AppImage

Requirements:
  - x86_64 CPU.
  - A Vulkan-capable GPU driver (Mesa RADV/ANV/NVK, or the proprietary NVIDIA
    driver with Vulkan support).
  - skate-convert (Skate 3's one-time asset setup) unpacks itself to
    $TMPDIR, or /tmp if TMPDIR is unset. If /tmp is mounted noexec, set
    TMPDIR to a directory that allows execution before running it.
EOF

echo "== appimagetool"
TOOL="$WORK/appimagetool-x86_64.AppImage"
if [ ! -x "$TOOL" ] || [ "$(sha256sum "$TOOL" | awk '{print $1}')" != "$APPIMAGETOOL_SHA256" ]; then
  curl -fL -o "$TOOL" "$APPIMAGETOOL_URL"
  ACTUAL_SHA256="$(sha256sum "$TOOL" | awk '{print $1}')"
  if [ "$ACTUAL_SHA256" != "$APPIMAGETOOL_SHA256" ]; then
    echo "error: appimagetool sha256 mismatch: expected $APPIMAGETOOL_SHA256, got $ACTUAL_SHA256" >&2
    exit 1
  fi
  chmod +x "$TOOL"
fi
OUT="$ROOT/dist/2010-Rust-Rewrite-Mashup-x86_64.AppImage"
APPIMAGETOOL_ARGS=()
if [ -n "${APPIMAGE_UPDATE_INFO:-}" ]; then
  APPIMAGETOOL_ARGS+=(-u "$APPIMAGE_UPDATE_INFO")
fi
ARCH=x86_64 "$TOOL" --appimage-extract-and-run "${APPIMAGETOOL_ARGS[@]}" "$APPDIR" "$OUT"
echo "built $OUT (glibc $(ldd --version | head -1 | awk '{print $NF}'))"
