# Security Policy

## Supported versions

Phast is pre-1.0 and under active development. Security fixes land on the
latest release only; there are no long-term support branches yet. Run
`phast cli update` to make sure you are on the current version.

## Reporting a vulnerability

**Please do not open a public issue for security problems.**

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/phast-sh/phast/security/advisories/new).
This opens a confidential advisory visible only to you and the maintainers, and
keeps the report, discussion, and fix in one place until a coordinated
disclosure.

If you cannot use GitHub advisories, email
[security@phast.sh](mailto:security@phast.sh) instead.

Please include:

- the affected version (`phast cli version`) and platform
- a description of the issue and its impact
- steps to reproduce, ideally a minimal `phast.toml` or command sequence
- any relevant logs or stack traces

## Scope

Phast installs packages from Packagist and other registries. A malicious or
vulnerable package is not a Phast vulnerability: report it to the package
maintainer or to the registry that serves it. Phast vulnerabilities are flaws
in Phast itself, for example in checksum verification, archive extraction,
script execution, or credential handling.

## What to expect

- an acknowledgement within a few days
- an assessment of severity and affected versions, and a fix or mitigation plan
- credit in the release notes and advisory once the fix ships, unless you
  prefer to stay anonymous

Please give me a reasonable window to release a fix before any public
disclosure. Thank you for helping keep Phast and its users safe.
