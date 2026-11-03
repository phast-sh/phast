//! Running a project's `[scripts]`.
//!
//! One side names the moments Phast fires a hook at (`post-install-cmd` and friends)
//! and dispatches to whatever the caller registered. The other takes a script entry,
//! works out whether it is a shell command, a `@php` invocation, or a PHP static
//! callback, and runs it. Neither side binds itself to the other, which is why a
//! command decides for itself which events it cares about.
//!
//! Scripts matter more than they look: a Laravel application is broken after install
//! unless `post-autoload-dump` runs `php artisan package:discover`, so executing hooks
//! correctly is a prerequisite for real projects rather than a convenience.

pub mod event;
pub mod executor;
pub mod php;
pub mod runner;

mod hints;
