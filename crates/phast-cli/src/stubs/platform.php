<?php

/**
 * Emit all loaded PHP extensions and platform virtual packages to stdout.
 *
 * Output format: one `{name}={version}` per line, suitable for parsing
 * by Phast's platform map builder.
 *
 * In addition to `ext-{name}` entries this script emits `php-64bit` or
 * `php-32bit` at the current PHP version. Composer does the same -- these
 * are the only commonly-required `php-*` virtual platform packages.
 */
foreach (get_loaded_extensions() as $ext) {
    $v = phpversion($ext);
    echo 'ext-' . strtolower($ext) . '=' . ($v !== false ? $v : PHP_VERSION) . PHP_EOL;
}

echo (PHP_INT_SIZE === 8 ? 'php-64bit' : 'php-32bit') . '=' . PHP_VERSION . PHP_EOL;
