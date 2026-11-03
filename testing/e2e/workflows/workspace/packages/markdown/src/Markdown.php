<?php

namespace Acme\Markdown;

class Markdown
{
    public function render(string $source): string
    {
        return "<p>{$source}</p>";
    }
}
