#!/usr/bin/env bash
#
# Phast against Composer on real projects, timed with hyperfine.
#
# Every project is a real repository, shallow-cloned at a pinned tag before anything is timed,
# and checked against the commit the tag pointed to when it was pinned. A tag can be moved,
# and a moved one fails the run rather than quietly measuring different code under the same
# name. Each tool installs its own copy with its own cache and scripts off, so the tool is the
# only difference. There is no phast.toml on purpose: Phast reads the same composer.json
# Composer does, so neither tool gets a manifest the other did not.
#
# Scenarios, each its own hyperfine comparison:
#   cold      no lock, no cache, no vendor/   the first install ever
#   lock      lock, no cache, no vendor/      a fresh CI runner
#   warm      lock and cache, no vendor/      CI with a restored cache
#   noop      everything already installed    install run again
#   resolve   cache, no lock, resolve only    phast lock, composer update --no-install
#
# Usage:
#   make test-bench                         every project, RUNS=5
#   make test-bench PROJECT=koel RUNS=10    targeted project, configured number of runs
#
# A project with a GitHub vcs repository (koel) needs GITHUB_TOKEN, or both tools
# run into GitHub's unauthenticated rate limit and the numbers measure that instead.

set -euo pipefail
cd "$(dirname "$0")"

RUNS=${RUNS:-5}
ROOT_DIR=$(cd ../.. && pwd)
PHAST=$ROOT_DIR/target/release/phast
OUT=results/$(date +%Y-%m-%d-%H%M)

fail() { echo "bench: $*" >&2; exit 1; }

# The command hyperfine runs before every run: remove the given paths, or nothing.
prepare() { [ -n "$1" ] && echo "rm -rf $1" || echo true; }

# Times one scenario for both tools. The paths given for each tool are removed before every
# run; none means the run starts from whatever the previous scenario left. Paths are not
# quoted inside the commands, which is safe because they all live under mktemp's directory.
measure() {
    local scenario=$1 action=$2 composer_removes=$3 phast_removes=$4
    local composer_run phast_run

    case $action in
        install) composer_run=$composer_install phast_run=$phast_install ;;
        resolve) composer_run=$composer_resolve phast_run=$phast_resolve ;;
    esac

    echo
    echo "== $project: $scenario"
    hyperfine --runs "$RUNS" --style basic \
        --command-name composer --prepare "$(prepare "$composer_removes")" "$composer_run" \
        --command-name phast    --prepare "$(prepare "$phast_removes")"    "$phast_run" \
        --export-json     "$OUT/$project-$scenario.json" \
        --export-markdown "$OUT/$project-$scenario.md"
}

# Checks out one project at its pinned tag and times every scenario on it. Skipped when
# PROJECT names a different one.
project() {
    local project=$1 repository=$2 tag=$3 commit=$4
    [ -z "${PROJECT:-}" ] || [ "$PROJECT" = "$project" ] || return 0
    measured=$((measured + 1))

    local workdir=$work_root/$project
    local checkout=$workdir/checkout
    mkdir "$workdir"

    git clone --quiet --depth 1 --branch "$tag" -c advice.detachedHead=false "$repository" "$checkout"
    local head
    head=$(git -C "$checkout" rev-parse HEAD)
    [ "$head" = "$commit" ] || fail "$project $tag now points at $head, not the pinned $commit"
    echo "project    $project $tag (${commit:0:7})" | tee -a "$OUT/environment.txt"

    # Without .git neither tool asks git for the root package's version, which is not what is
    # being measured, and each copy is the source alone.
    rm -rf "$checkout/.git"

    # roave/security-advisories is only conflict rules, updated daily, so a new advisory can make
    # a pinned project unresolvable overnight. Both copies drop it before anything is timed.
    composer remove --dev --no-update --no-interaction --quiet --working-dir "$checkout" roave/security-advisories

    local composer_dir=$workdir/composer
    local composer_vendor=$composer_dir/vendor
    local composer_lock=$composer_dir/composer.lock
    local composer_cache=$workdir/composer-cache

    local phast_dir=$workdir/phast
    local phast_vendor=$phast_dir/vendor
    local phast_lock=$phast_dir/phast.lock
    local phast_cache=$workdir/phast-cache

    cp -R "$checkout" "$composer_dir"
    cp -R "$checkout" "$phast_dir"

    export COMPOSER_CACHE_DIR=$composer_cache
    export COMPOSER_HOME=$workdir/composer-home

    # The token reaches each tool the way it reads one, and never leaves $workdir.
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        export COMPOSER_AUTH="{\"github-oauth\":{\"github.com\":\"$GITHUB_TOKEN\"}}"
        printf '[github-token]\n"github.com" = "%s"\n' "$GITHUB_TOKEN" > "$phast_dir/auth.toml"
    fi

    local composer_install="cd $composer_dir && composer install --no-interaction --no-progress --no-scripts --no-plugins --optimize-autoloader --quiet"
    local composer_resolve="cd $composer_dir && composer update --no-install --no-interaction --no-progress --no-scripts --no-plugins --no-audit --quiet"

    local phast_install="cd $phast_dir && $PHAST install --no-scripts --quiet --cache-dir $phast_cache"
    local phast_resolve="cd $phast_dir && $PHAST lock --quiet --cache-dir $phast_cache"

    # In this order on purpose: every install scenario ends fully installed, which is exactly
    # the lock, cache or vendor/ the next one starts from.
    #
    #        usecase  |  action  |         composer removes before each run         |         phast removes before each run         |
    measure   cold      install   "$composer_vendor $composer_lock $composer_cache"   "$phast_vendor $phast_lock $phast_cache"
    measure   lock      install   "$composer_vendor $composer_cache"                  "$phast_vendor $phast_cache"
    measure   warm      install   "$composer_vendor"                                  "$phast_vendor"
    measure   noop      install   ""                                                  ""
    measure   resolve   resolve   "$composer_lock"                                    "$phast_lock"
}

command -v php       >/dev/null || fail "php is not installed"
command -v composer  >/dev/null || fail "composer is not installed"
command -v hyperfine >/dev/null || fail "hyperfine is not installed"

# A debug binary is several times slower, and a number from it means nothing here.
cargo build --locked --release -p phast --quiet

# A slow or failing Packagist skews the two tools differently, so measure nothing then.
curl -fsS -o /dev/null --max-time 5 https://repo.packagist.org/p2/psr/log.json \
    || fail "Packagist is not answering, so any number measured now would be noise"

mkdir -p "$OUT"
{
    echo "date       $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "machine    $(uname -sm), $(sysctl -n machdep.cpu.brand_string 2>/dev/null || grep -m1 'model name' /proc/cpuinfo | cut -d: -f2-)"
    echo "phast      $("$PHAST" --version) ($(git -C "$ROOT_DIR" rev-parse --short HEAD))"
    echo "composer   $(composer --version --no-ansi 2>/dev/null | head -1)"
    echo "php        $(php -r 'echo PHP_VERSION;')"
    echo "runs       $RUNS"
} > "$OUT/environment.txt"
cat "$OUT/environment.txt"

work_root=$(mktemp -d)
trap 'rm -rf "$work_root"' EXIT

measured=0

# Pinned projects. The commit is the one the tag pointed to when it was pinned.
#
#     |  project  |             repository             |    tag    |                  commit                  |
project   koel       https://github.com/koel/koel         v9.15.0    7f9fe1ad76aee28d5d2a808ea91890d4ef866b0b
project   laravel    https://github.com/laravel/laravel   v13.10.1   5aad4ddf34d5e21dfe6b4c07eeac67d5bd5e08b0

[ "$measured" -gt 0 ] || fail "no project named '${PROJECT:-}'"

# One table across every project and scenario.
# summary.py also pools several runs.
python3 summary.py "$OUT" | tee "$OUT/summary.md"
