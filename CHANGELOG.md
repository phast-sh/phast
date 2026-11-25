# Changelog

## [0.1.1](https://github.com/phast-sh/phast/releases/tag/v0.1.1)

Released on 2026-11-25.

### Bug fixes

- Suggest `phast migrate` as well as `init` when phast.toml is missing ([#14](https://github.com/phast-sh/phast/pull/14))
- Hide the cursor only when stderr is a terminal, so pipes and CI logs get no escape codes ([#15](https://github.com/phast-sh/phast/pull/15))
- Don't fall through to a later registry when `minimum-package-age` holds back the owner's releases ([#16](https://github.com/phast-sh/phast/pull/16))
- Remove packages that left the lock from `vendor/` ([#18](https://github.com/phast-sh/phast/pull/18))
- Fetch each VCS commit's manifest once per resolve, not again on every pass ([#20](https://github.com/phast-sh/phast/pull/20))
- Resolve a package that lists itself under `replace/provide` ([#29](https://github.com/phast-sh/phast/pull/29)), reported by @TBillekens in [#26](https://github.com/phast-sh/phast/issues/26)
- Resolve virtual packages only after every real package is read ([#32](https://github.com/phast-sh/phast/pull/32)), reported by @TBillekens in [#27](https://github.com/phast-sh/phast/issues/27)
- Read map scripts, and run other scripts with `@name` ([#33](https://github.com/phast-sh/phast/pull/33)), reported by @craigrileyuk in [#31](https://github.com/phast-sh/phast/issues/31)
- Resolve `self.version` in registry package requirements ([#35](https://github.com/phast-sh/phast/pull/35)), reported by @PiahDoNeumann in [#22](https://github.com/phast-sh/phast/issues/22)
- Pass the project's data to PHP script callbacks ([#38](https://github.com/phast-sh/phast/pull/38)), reported by @pyrou in [#21](https://github.com/phast-sh/phast/issues/21)

## [0.1.0](https://github.com/phast-sh/phast/releases/tag/v0.1.0)

Released on 2026-11-03.

Initial public release (beta). See the [release notes](https://github.com/phast-sh/phast/releases/tag/v0.1.0) for details.
