<?php

/*
 * This file is part of Phast.
 *
 * (c) Denys Horobtsov <0xbedrock@gmail.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 *
 * Based on Composer's InstalledVersions
 * (c) Nils Adermann <naderman@naderman.de>, Jordi Boggiano <j.boggiano@seld.be>
 */

namespace Phast;

/**
 * Runtime introspection for installed packages.
 *
 * Drop-in replacement for \Composer\InstalledVersions. Registered under both
 * \Phast\InstalledVersions (canonical) and \Composer\InstalledVersions (alias)
 * by autoload_real.php.
 *
 * Data is loaded lazily from vendor/phast/installed.php on first access.
 * opcache serves the pre-built array without file I/O or JSON parsing.
 */
class InstalledVersions
{
    /** @var array<string, mixed>|null Raw data from installed.php, null until first load. */
    private static ?array $data = null;

    /**
     * Load installed.php on the first call; subsequent calls are no-ops.
     */
    private static function load(): void
    {
        if (! is_null(self::$data)) {
            return;
        }

        $file = __DIR__ . '/installed.php';
        if (! file_exists($file)) {
            self::$data = ['root' => [], 'versions' => [], 'dev' => false];

            return;
        }

        /** @var array<string, mixed> $loaded */
        $loaded = require $file;
        self::$data = $loaded;
    }

    /**
     * Return true if the given package is installed (production or dev).
     */
    public static function isInstalled(string $packageName): bool
    {
        self::load();

        return isset(self::$data['versions'][$packageName]);
    }

    /**
     * Return the normalized version string (e.g. "1.2.3.0"), or null if not installed.
     */
    public static function getVersion(string $packageName): ?string
    {
        self::load();

        return self::$data['versions'][$packageName]['version'] ?? null;
    }

    /**
     * Return the human-readable version string (e.g. "1.2.3"), or null if not installed.
     */
    public static function getPrettyVersion(string $packageName): ?string
    {
        self::load();

        return self::$data['versions'][$packageName]['pretty_version'] ?? null;
    }

    /**
     * Return the VCS reference (commit hash or tag) the package was installed from,
     * or null when unavailable (e.g. dist-only installs without a reference).
     */
    public static function getReference(string $packageName): ?string
    {
        self::load();

        return self::$data['versions'][$packageName]['reference'] ?? null;
    }

    /**
     * Return the absolute path to the package's installation directory,
     * or null if the package is not installed.
     */
    public static function getInstallPath(string $packageName): ?string
    {
        self::load();

        return self::$data['versions'][$packageName]['install_path'] ?? null;
    }

    /**
     * Return the names of all installed packages (production and, if present, dev).
     *
     * @return list<string>
     */
    public static function getInstalledPackages(): array
    {
        self::load();

        return array_keys(self::$data['versions'] ?? []);
    }

    /**
     * Return the names of all installed packages with the given Composer type
     * (e.g. "library", "composer-plugin").
     *
     * @return list<string>
     */
    public static function getInstalledPackagesByType(string $type): array
    {
        self::load();

        $names = [];
        foreach (self::$data['versions'] ?? [] as $name => $package) {
            if (($package['type'] ?? 'library') === $type) {
                $names[] = $name;
            }
        }

        return $names;
    }

    /**
     * Return root package metadata as recorded in installed.php.
     *
     * @return array<string, mixed>
     */
    public static function getRootPackage(): array
    {
        self::load();

        return self::$data['root'] ?? [];
    }

    /**
     * Return true if the installed version of $packageName satisfies $constraint.
     *
     * Delegates parsing to the provided VersionParser so that Composer's full
     * constraint language (ranges, wildcards, stability flags) is supported
     * without reimplementing it here.
     *
     * @param \Composer\Semver\VersionParser $parser
     */
    public static function satisfies($parser, string $packageName, string $constraint): bool
    {
        self::load();

        $version = self::$data['versions'][$packageName]['version'] ?? null;
        if (is_null($version)) {
            return false;
        }

        // Use Semver::satisfies() when composer/semver is on the autoload path,
        // which it almost always is since the caller already has a VersionParser.
        if (class_exists(\Composer\Semver\Semver::class)) {
            return \Composer\Semver\Semver::satisfies($version, $constraint);
        }

        // Fallback: exact string match only (covers the common "check own version" pattern).
        return $version === $constraint;
    }

    /**
     * Return the raw contents of installed.php.
     *
     * @return array<string, mixed>
     */
    public static function getRawData(): array
    {
        self::load();

        return self::$data ?? [];
    }
}
