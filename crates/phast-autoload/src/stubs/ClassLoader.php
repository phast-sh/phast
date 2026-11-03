<?php

/*
 * This file is part of Phast.
 *
 * (c) Denys Horobtsov <0xbedrock@gmail.com>
 *
 * For the full copyright and license information, please view the LICENSE
 * file that was distributed with this source code.
 *
 * Based on Composer's ClassLoader
 * (c) Nils Adermann <naderman@naderman.de>, Jordi Boggiano <j.boggiano@seld.be>
 */

namespace Phast\Autoload;

/**
 * \Phast\Autoload\ClassLoader -- PSR-4, PSR-0, classmap, and files autoloader.
 *
 * \Composer\Autoload\ClassLoader is registered as an alias in autoload_real.php
 * for compatibility with packages that reference the Composer class directly.
 */
class ClassLoader
{
    /**
     * @var array<string, string> class name -> file path (the fast path)
     */
    private array $classMap = [];

    /**
     * When true, the classmap is the whole truth: a class absent from it does
     * not exist, and the PSR filesystem fallback is skipped entirely. For
     * immutable production trees; classes generated at runtime stop loading.
     */
    private bool $classMapAuthoritative = false;

    /**
     * @var array<string, list<string>> PSR-4 prefix -> base dirs
     */
    private array $psr4Dirs = [];

    /**
     * PSR-4 prefixes indexed by their first character, so a lookup only scans
     * prefixes that can possibly match the class name.
     *
     * @var array<string, array<string, int>> first char -> (prefix -> prefix length)
     */
    private array $psr4Index = [];

    /**
     * @var array<string, list<string>> PSR-0 prefix -> base dirs
     */
    private array $psr0Dirs = [];

    /**
     * Classes already looked up and not found, so a repeated miss costs one
     * array access instead of the full prefix walk and filesystem probes.
     * Never persisted; lives only for the current request.
     *
     * @var array<string, true>
     */
    private array $missingClasses = [];

    // -------------------------------------------------------------------------
    // Registration
    // -------------------------------------------------------------------------

    /**
     * Register this autoloader with the SPL autoload stack.
     *
     * @param bool $prepend When true, prepend to the stack so this loader runs first.
     */
    public function register(bool $prepend = false): void
    {
        spl_autoload_register([$this, 'loadClass'], true, $prepend);
    }

    /**
     * Unregister this autoloader from the SPL autoload stack.
     */
    public function unregister(): void
    {
        spl_autoload_unregister([$this, 'loadClass']);
    }

    // -------------------------------------------------------------------------
    // PSR-0
    // -------------------------------------------------------------------------

    /**
     * Replace the PSR-0 base directories for a given prefix.
     *
     * @param string       $prefix The PSR-0 namespace prefix.
     * @param array|string $paths  One or more base directories.
     */
    public function set(string $prefix, array|string $paths): void
    {
        $this->psr0Dirs[$prefix] = (array) $paths;
    }

    /**
     * Add base directories for a PSR-0 prefix, optionally prepending them.
     *
     * @param string       $prefix  The PSR-0 namespace prefix.
     * @param array|string $paths   One or more base directories to add.
     * @param bool         $prepend When true, the new dirs are searched before existing ones.
     */
    public function add(string $prefix, array|string $paths, bool $prepend = false): void
    {
        $paths = (array) $paths;
        $existing = $this->psr0Dirs[$prefix] ?? [];
        $this->psr0Dirs[$prefix] = $prepend
            ? array_merge($paths, $existing)
            : array_merge($existing, $paths);
    }

    // -------------------------------------------------------------------------
    // PSR-4
    // -------------------------------------------------------------------------

    /**
     * Replace the PSR-4 base directories for a given namespace prefix.
     *
     * @param string       $prefix The PSR-4 namespace prefix (must end with `\\`).
     * @param array|string $paths  One or more base directories.
     */
    public function setPsr4(string $prefix, array|string $paths): void
    {
        $paths = (array) $paths;
        if ($prefix === '') {
            $this->psr4Dirs[''] = $paths;
        } else {
            $this->psr4Dirs[$prefix] = $paths;
            $this->psr4Index[$prefix[0]][$prefix] = strlen($prefix);
        }
    }

    /**
     * Add base directories for a PSR-4 namespace prefix, optionally prepending them.
     *
     * @param string       $prefix  The PSR-4 namespace prefix (must end with `\\`).
     * @param array|string $paths   One or more base directories to add.
     * @param bool         $prepend When true, the new dirs are searched before existing ones.
     */
    public function addPsr4(string $prefix, array|string $paths, bool $prepend = false): void
    {
        $paths = (array) $paths;
        if ($prefix === '') {
            $existing = $this->psr4Dirs[''] ?? [];
            $this->psr4Dirs[''] = $prepend
                ? array_merge($paths, $existing)
                : array_merge($existing, $paths);
        } else {
            $existing = $this->psr4Dirs[$prefix] ?? [];
            $this->psr4Dirs[$prefix] = $prepend
                ? array_merge($paths, $existing)
                : array_merge($existing, $paths);
            $this->psr4Index[$prefix[0]][$prefix] = strlen($prefix);
        }
    }

    // -------------------------------------------------------------------------
    // Classmap
    // -------------------------------------------------------------------------

    /**
     * Merge additional class-to-file mappings into the classmap.
     *
     * @param array<string, string> $classMap Fully-qualified class name -> absolute file path.
     */
    public function addClassMap(array $classMap): void
    {
        $this->classMap = array_merge($this->classMap, $classMap);
    }

    /**
     * Return the current classmap.
     *
     * @return array<string, string> Fully-qualified class name -> absolute file path.
     */
    public function getClassMap(): array
    {
        return $this->classMap;
    }

    /**
     * Turn authoritative mode on or off.
     *
     * When on, only classmap entries load: a class absent from the map is
     * definitively not found and the PSR filesystem fallback never runs.
     *
     * @param bool $classMapAuthoritative True to trust the classmap exclusively.
     */
    public function setClassMapAuthoritative(bool $classMapAuthoritative): void
    {
        $this->classMapAuthoritative = $classMapAuthoritative;
    }

    /**
     * Whether the classmap is trusted exclusively (no filesystem fallback).
     */
    public function isClassMapAuthoritative(): bool
    {
        return $this->classMapAuthoritative === true;
    }

    // -------------------------------------------------------------------------
    // Class loading
    // -------------------------------------------------------------------------

    /**
     * Load the file for a given class name.
     *
     * Called by the SPL autoload stack. Returns true on success, null when the
     * class cannot be found so the next registered autoloader can try.
     *
     * @param string $class Fully-qualified class name.
     */
    public function loadClass(string $class): bool|null
    {
        $file = $this->findFile($class);
        if ($file !== false) {
            includeFile($file);

            return true;
        }

        return null;
    }

    /**
     * Find the file path for a given class name.
     *
     * Tries classmap first (O(1)), then PSR-4, then PSR-0.
     * Returns `false` on a miss:
     *      This is a public method external tooling calls
     *      directly and compares against with `=== false`,
     *      so a string-or-false return is the contract callers rely on.
     *
     * @param string $class Fully-qualified class name.
     */
    public function findFile(string $class): string|false
    {
        // Classmap lookup (fastest path).
        if (isset($this->classMap[$class])) {
            return $this->classMap[$class];
        }

        // In authoritative mode the classmap is the whole truth; a repeated
        // miss is answered from the negative cache without another walk.
        if ($this->isClassMapAuthoritative() || isset($this->missingClasses[$class])) {
            return false;
        }

        // PSR-4 lookup.
        $file = $this->findFilePsr4($class);
        if (! is_null($file)) {
            return $file;
        }

        // PSR-0 lookup.
        $file = $this->findFilePsr0($class);
        if (! is_null($file)) {
            return $file;
        }

        // Not found: remember, so the next lookup for this class is O(1).
        $this->missingClasses[$class] = true;

        return false;
    }

    /**
     * Locate a class file using PSR-4 prefix matching.
     *
     * Only prefixes sharing the class name's first character are scanned
     * (via the first-character index); each match strips the prefix from the
     * logical path and probes its base dirs.
     *
     * @param string $class Fully-qualified class name.
     */
    private function findFilePsr4(string $class): ?string
    {
        $logicalPath = strtr($class, '\\', DIRECTORY_SEPARATOR) . '.php';

        foreach ($this->psr4Index[$class[0]] ?? [] as $prefix => $length) {
            if (strncmp($class, $prefix, $length) !== 0) {
                continue;
            }

            $relPath = substr($logicalPath, $length);
            foreach ($this->psr4Dirs[$prefix] as $dir) {
                $file = $dir . DIRECTORY_SEPARATOR . $relPath;
                if (file_exists($file)) {
                    return $file;
                }
            }
        }

        // Fallback (empty prefix).
        if (isset($this->psr4Dirs[''])) {
            foreach ($this->psr4Dirs[''] as $dir) {
                $file = $dir . DIRECTORY_SEPARATOR . $logicalPath;
                if (file_exists($file)) {
                    return $file;
                }
            }
        }

        return null;
    }

    /**
     * Locate a class file using PSR-0 prefix matching.
     *
     * Both namespace separators (`\`) and underscores in the class name are
     * converted to directory separators, per the PSR-0 specification.
     *
     * @param string $class Fully-qualified class name.
     */
    private function findFilePsr0(string $class): ?string
    {
        $logicalPath = strtr($class, ['\\' => DIRECTORY_SEPARATOR, '_' => DIRECTORY_SEPARATOR]) . '.php';

        foreach ($this->psr0Dirs as $prefix => $dirs) {
            if ($prefix !== '' && strncmp($class, $prefix, strlen($prefix)) !== 0) {
                continue;
            }

            foreach ($dirs as $dir) {
                $file = $dir . DIRECTORY_SEPARATOR . $logicalPath;
                if (file_exists($file)) {
                    return $file;
                }
            }
        }

        return null;
    }
}

/**
 * Include a file in an isolated scope to prevent local variable leakage.
 *
 * Wrapping the include in a standalone function ensures that the included
 * file's top-level code has no access to the autoloader's internal variables.
 * A method would also expose `$this`; a plain function does not.
 *
 * @param string $file Absolute path to the file to include.
 */
function includeFile(string $file): void
{
    include $file;
}
