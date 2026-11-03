<?php

namespace Acme\Blog;

use Acme\Markdown\Markdown;

// Depends on the sibling member, which is what the shared autoloader has to make work.
class Blog
{
    public function __construct(private Markdown $markdown) { }
}
