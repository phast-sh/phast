<?php

namespace Acme\Hooks;

use Composer\Script\Event;

// Callbacks written for Composer, the way real packages write them.
class Report
{
    // Prints what it sees, then deletes a class, like the AWS SDK cleanup does.
    public static function show(Event $event): void
    {
        $io       = $event->getIO();
        $composer = $event->getComposer();
        $package  = $composer->getPackage();
        $config   = $composer->getConfig();

        // Phast doesn't keep the key order of `extra` yet, so sort it to compare.
        $extra = $package->getExtra();
        ksort($extra['acme-hooks']);

        $io->write('event: '      . $event->getName());
        $io->write('dev: '        . ($event->isDevMode() ? 'yes' : 'no'));
        $io->write('package: '    . $package->getName());
        $io->write('extra: '      . json_encode($extra, JSON_UNESCAPED_SLASHES));
        $io->write('vendor-dir: ' . basename($config->get('vendor-dir')));

        $unused = __DIR__ . '/../legacy/Unused.php';
        if (is_file($unused)) {
            unlink($unused);
        }
    }

    // Asks for something Phast doesn't provide.
    public static function locker(Event $event): void
    {
        $event->getComposer()->getLocker();
    }
}
