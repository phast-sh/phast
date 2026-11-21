<?php

// Prints whether `Unused` class can still be loaded after the callback deleted it.
require 'vendor/autoload.php';

echo class_exists('Acme\Hooks\Unused') ? 'present' : 'missing', PHP_EOL;
