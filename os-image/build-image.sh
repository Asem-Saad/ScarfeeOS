#!/usr/bin/env bash
# Build the ScarfeeOS live ISO with kiwi. Run as root on Fedora (x86_64).
# NOT RUN YET: no image has ever been built (see README.md, "Status").

set -euo pipefail

# Estimates from the Fedora 44 package metadata (445 packages, about 550 MiB
# to download and 1.8 GiB installed) plus room for the squashfs, the ISO tree
# and the ISO. Not measured on a real build.
NEED_OUTPUT_GIB=10
NEED_CACHE_GIB=2
CACHE_DIR=/var/cache/kiwi

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

usage() {
    cat <<EOF
Usage: sudo $0 OUTPUT_DIR

Builds the ScarfeeOS live ISO from $HERE/config.xml.

  OUTPUT_DIR   Where the build goes. Created if missing. It must be on a
               disk-backed filesystem, not under $CACHE_DIR, and must not hold
               a build/ directory from an earlier run.

Output (all inside OUTPUT_DIR):
  ScarfeeOS.x86_64-*.iso            the live ISO
  ScarfeeOS.x86_64-*.iso.sha256     its checksum
  kiwi.log                          the kiwi debug log
  build/                            the kiwi work tree; safe to delete afterwards

Disk needed (estimated, not measured): about ${NEED_OUTPUT_GIB} GiB free in OUTPUT_DIR,
${NEED_CACHE_GIB} GiB free in $CACHE_DIR (downloaded packages), and a few MiB in
$HERE/repo (the three scarfeeos-* RPMs this script builds). It downloads about
550 MiB from dl.fedoraproject.org. Expect tens of minutes.

Prerequisites are checked first and all missing ones are listed. On Fedora:
  dnf install kiwi-cli kiwi-systemdeps-core kiwi-systemdeps-filesystems \\
      kiwi-systemdeps-bootloaders kiwi-systemdeps-iso-media rpm-build \\
      createrepo_c curl
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi
if [ "$#" -ne 1 ] || [ -z "$1" ]; then
    usage >&2
    exit 2
fi
OUT=$(realpath -m -- "$1")

problems=()
problem() { problems+=("$1"); }

nearest_existing() {
    local p=$1
    while [ ! -e "$p" ]; do p=$(dirname -- "$p"); done
    printf '%s\n' "$p"
}

free_gib() {
    local kib
    kib=$(df -Pk -- "$(nearest_existing "$1")" | awk 'NR==2 {print $4}')
    echo $((kib / 1024 / 1024))
}

if [ "$(id -u)" -ne 0 ]; then
    problem "must run as root (kiwi mounts and chroots); try: sudo $0 $1"
fi

if [ -r /etc/os-release ]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    if [ "${ID:-}" != "fedora" ]; then
        problem "this is not Fedora (ID=${ID:-unknown}); run it on Fedora, in a privileged container or VM if needed"
    elif [ "${VERSION_ID:-0}" -lt 43 ] 2>/dev/null; then
        problem "Fedora ${VERSION_ID} is too old; use Fedora 43 or newer (the image itself is Fedora 44)"
    fi
else
    problem "cannot read /etc/os-release"
fi

if [ "$(uname -m)" != "x86_64" ]; then
    problem "host is $(uname -m); the image is x86_64 only"
fi

for tool in kiwi-ng rpmbuild createrepo_c dnf5 curl xorriso mksquashfs mkfs.fat mcopy; do
    command -v "$tool" >/dev/null 2>&1 || problem "missing command: $tool"
done

if command -v kiwi-ng >/dev/null 2>&1; then
    # kiwi-ng --version exits 1 on 11.1.1, so ignore its status
    kiwi_version=$( (kiwi-ng --version 2>/dev/null || true) | sed -n 's/.*version \([0-9][0-9.]*\).*/\1/p' | head -n 1)
    if [ -z "$kiwi_version" ]; then
        problem "cannot read the kiwi version from 'kiwi-ng --version'"
    elif [ "${kiwi_version%%.*}" -lt 10 ]; then
        problem "kiwi $kiwi_version is too old for config.xml (tested with 11.1.1, the version in Fedora 44)"
    fi
fi

for f in config.xml packages.txt; do
    [ -f "$HERE/$f" ] || problem "missing $HERE/$f"
done
for d in scarfeeos-release scarfeeos-repos scarfeeos-logos; do
    [ -f "$HERE/$d/$d.spec" ] || problem "missing $HERE/$d/$d.spec"
done

case "$OUT/" in
    "$CACHE_DIR"/*) problem "OUTPUT_DIR must not be under $CACHE_DIR (kiwi refuses it)" ;;
esac
if [ -e "$OUT" ] && [ ! -d "$OUT" ]; then
    problem "$OUT exists and is not a directory"
elif [ -d "$OUT/build" ]; then
    problem "$OUT/build exists from an earlier run; delete it or choose another OUTPUT_DIR"
fi
fstype=$(stat -f -c %T -- "$(nearest_existing "$OUT")")
case "$fstype" in
    tmpfs | ramfs | nfs* | cifs | smb* | overlayfs | overlay)
        problem "OUTPUT_DIR is on a $fstype filesystem; use a local disk (ext4, xfs or btrfs)" ;;
esac
have=$(free_gib "$OUT")
[ "$have" -ge "$NEED_OUTPUT_GIB" ] || problem "OUTPUT_DIR has $have GiB free, needs about $NEED_OUTPUT_GIB GiB"
have=$(free_gib "$CACHE_DIR")
[ "$have" -ge "$NEED_CACHE_GIB" ] || problem "$CACHE_DIR has $have GiB free, needs about $NEED_CACHE_GIB GiB"

if command -v curl >/dev/null 2>&1 && [ -f "$HERE/config.xml" ]; then
    mapfile -t urls < <(sed -n 's/.*<source path="\(https:\/\/[^"]*\)".*/\1/p' "$HERE/config.xml")
    [ "${#urls[@]}" -gt 0 ] || problem "found no https repositories in config.xml"
    for url in "${urls[@]}"; do
        curl -fsSIL --max-time 30 -o /dev/null "${url%/}/repodata/repomd.xml" \
            || problem "cannot reach $url (network, or the release moved to the archive)"
    done
fi

if [ "${#problems[@]}" -gt 0 ]; then
    echo "Prerequisites not met:" >&2
    printf '  - %s\n' "${problems[@]}" >&2
    echo >&2
    echo "Nothing was changed. Run '$0 --help' for the install command." >&2
    exit 1
fi

echo "Prerequisites OK (kiwi $kiwi_version, Fedora ${VERSION_ID})."
echo "Output goes to:    $OUT"
echo "Disk needed:       about $NEED_OUTPUT_GIB GiB in the output directory, $NEED_CACHE_GIB GiB in $CACHE_DIR (estimates)"
echo

mkdir -p -- "$OUT"
trap 'echo "Build failed. See $OUT/kiwi.log" >&2' ERR

echo "== Building the scarfeeos-* RPMs into $HERE/repo"
rm -rf -- "$HERE/repo"
mkdir -p -- "$HERE/repo"
topdir=$(mktemp -d)
trap 'rm -rf -- "$topdir"' EXIT
for name in scarfeeos-release scarfeeos-repos scarfeeos-logos; do
    mkdir -p -- "$topdir/$name"
    rpmbuild -bb \
        --define "_topdir $topdir/$name" \
        --define "_sourcedir $HERE/$name" \
        --define "_rpmdir $HERE/repo" \
        "$HERE/$name/$name.spec"
done
createrepo_c "$HERE/repo"

echo
echo "== Running kiwi (this takes a while; the log is $OUT/kiwi.log)"
kiwi-ng --debug --logfile "$OUT/kiwi.log" --type iso \
    system build --description "$HERE" --target-dir "$OUT"

shopt -s nullglob
isos=("$OUT"/*.iso)
if [ "${#isos[@]}" -eq 0 ]; then
    echo "kiwi finished but left no .iso in $OUT" >&2
    exit 1
fi
for iso in "${isos[@]}"; do
    (cd -- "$OUT" && sha256sum -- "$(basename -- "$iso")" > "$(basename -- "$iso").sha256")
    echo
    echo "Built: $iso ($(du -h -- "$iso" | cut -f 1))"
    cat -- "$iso.sha256"
done
echo
echo "Next: the boot-test checklists in $HERE/README.md (QEMU first, then a real laptop)."
