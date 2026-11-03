<?php

namespace Acme\Blog\Tests;

// Reachable only through the [autoload-dev] rules, which is what makes it the probe for
// whether a dump included them.
class Helper
{
    public function name(): string
    {
        return 'dev helper';
    }
}
