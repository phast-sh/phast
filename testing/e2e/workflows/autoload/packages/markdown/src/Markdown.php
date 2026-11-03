<?php

namespace Acme\Markdown;

// Autoloaded through the psr-4 prefix this package declares. Nothing in the workflow
// executes it yet; it exists so the prefix maps to a directory that has a class in it.
class Markdown
{
    public function render(string $source): string
    {
        return "<p>{$source}</p>";
    }
}
