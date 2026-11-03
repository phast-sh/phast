//! Manifest-editing helpers shared by commands that modify `phast.toml`.
//!
//! Loading and format discovery live on [`phast_core::manifest::Manifest::load`], which
//! answers what a manifest says. Editing needs the opposite: a loaded `Manifest` cannot be
//! written back without flattening the file's comments, spacing and key order, so everything
//! here works on the raw TOML document instead.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml_edit::DocumentMut;
use toml_edit::InlineTable;

/// Which section of `phast.toml` a dependency belongs to.
#[derive(Debug, Clone, Copy)]
pub enum Section {
    Require,
    RequireDev,
}

#[rustfmt::skip]
impl Section {
    /// The TOML table key for this section.
    pub fn as_str(&self) -> &'static str {
        match self {
            Section::Require    => "require",
            Section::RequireDev => "require-dev",
        }
    }

    /// The other section, which is what the wrong-section check asks about.
    pub fn opposite(&self) -> Self {
        match self {
            Section::Require    => Section::RequireDev,
            Section::RequireDev => Section::Require,
        }
    }
}

/// What a dependency section records for a package right now.
///
/// Absent and inherited are told apart because they look identical to a string lookup and
/// mean opposite things to a change: writing over nothing adds a dependency, writing over
/// `{ workspace = true }` writes the same bytes back.
pub enum Declared {
    /// The section does not carry the package.
    Absent,
    /// A version constraint the manifest spells out.
    Constraint(String),
    /// `{ workspace = true }`: whatever the workspace root pins.
    Inherited,
}

/// One edit to a dependency section, already validated against the manifest.
///
/// Commands plan every change before applying any, so a request that cannot be satisfied
/// fails while the document is still untouched. Producing a change *is* the validation:
/// there is no way to apply one that was not planned.
pub enum Change {
    /// Record `constraint`, replacing whatever the section held for this package.
    Insert {
        /// The `vendor/package` name.
        package: String,
        /// The constraint to record.
        constraint: String,
        /// What the section recorded before this change.
        replace: Declared,
    },
    /// Record `{ workspace = true }`, deferring the constraint to the workspace root.
    Inherit {
        /// The `vendor/package` name.
        package: String,
        /// What the section recorded before this change.
        replace: Declared,
    },
    /// Drop the package from the section.
    Remove {
        /// The `vendor/package` name.
        package: String,
    },
}

#[rustfmt::skip]
impl Change {
    /// The package this change is about.
    pub fn package(&self) -> &str {
        match self {
              Change::Insert  { package, .. }
            | Change::Inherit { package, .. }
            | Change::Remove  { package,    } => package,
        }
    }

    /// Whether applying this change would write what the section already says.
    pub fn is_noop(&self) -> bool {
        match self {
            Change::Insert { constraint, replace, .. } => {
                matches!(replace, Declared::Constraint(c) if c == constraint)
            },
            Change::Inherit { replace, .. } => {
                matches!(replace, Declared::Inherited)
            },
            Change::Remove { .. } => false,
        }
    }
}

/// The dependency sections of a `phast.toml`, opened for editing.
///
/// The only thing in the CLI that knows the manifest is TOML. Commands ask what a section
/// records and say what it should record instead; formatting, comments, and key order are
/// preserved because the document is edited rather than rewritten.
///
/// Edits are held in memory until [`save`](Self::save), so a command that fails partway
/// leaves the file untouched.
pub struct Editor {
    /// Absolute path to the manifest being edited.
    path: PathBuf,
    /// The parsed document, carrying the formatting of the file as read.
    document: DocumentMut,
}

impl Editor {
    /// Open `dir/phast.toml` for editing.
    ///
    /// Whether the file ought to be there is the caller's question: commands check for it
    /// and report it in their own words.
    #[rustfmt::skip]
    pub fn open(dir: &Path) -> Result<Self> {
        let manifest = dir.join("phast.toml");
        let content = std::fs::read_to_string(&manifest)
            .context("failed to read phast.toml")?;
        let document: DocumentMut = content
            .parse()
            .context("failed to parse phast.toml")?;

        Ok(Self { path: manifest, document })
    }

    /// Whether `section` declares `package` at all, whatever form the value takes.
    ///
    /// Answers presence and nothing else, which is all the wrong-section check needs. Use
    /// [`declares`](Self::declares) when the form of the declaration matters.
    pub fn contains(&self, section: Section, package: &str) -> bool {
        self.document
            .get(section.as_str())
            .and_then(|table| table.get(package))
            .is_some()
    }

    /// What `section` records for `package` today.
    ///
    /// A dependency is either a constraint string or `{ workspace = true }`, so anything
    /// that is not a string is read as inherited. A section holding some other inline
    /// table is already unloadable, and reporting it here would not be what fixes it.
    pub fn declares(&self, section: Section, package: &str) -> Declared {
        let Some(value) = self
            .document
            .get(section.as_str())
            .and_then(|table| table.get(package))
        else {
            return Declared::Absent;
        };

        match value.as_str() {
            Some(c) => Declared::Constraint(c.to_owned()),
            None => Declared::Inherited,
        }
    }

    /// Apply every change to `section`.
    ///
    /// Takes the whole set rather than one at a time: the changes were validated together,
    /// and applying them together is what keeps a partly-edited document from ever
    /// existing.
    #[rustfmt::skip]
    pub fn apply(&mut self, section: Section, changes: &[Change]) {
        for change in changes {
            match change {
                Change::Insert { package, constraint, .. } => {
                    self.insert(section, package, constraint)
                },
                Change::Inherit { package, .. } => {
                    self.inherit(section, package)
                },
                Change::Remove { package } => {
                    self.remove(section, package)
                },
            }
        }
    }

    /// Write the edited document back over the file it was read from.
    #[rustfmt::skip]
    pub fn save(&self) -> Result<()> {
        std::fs::write(
            &self.path,
            self.document.to_string()
        ).context("failed to write phast.toml")
    }

    /// Whether this manifest is a workspace root, which declares no dependencies of its own.
    ///
    /// A `[workspace]` section is what makes one, the same test discovery applies as it walks
    /// upward looking for a root. Asked of the document rather than the loader so that
    /// deciding where a dependency may go needs no second read of the project.
    pub fn is_workspace_root(&self) -> bool {
        self.document.contains_key("workspace")
    }

    /// Record `constraint` for `package` in `section`, replacing any existing value.
    // TODO: append at the end of the section. When `config.sort-packages` is on,
    //       sorted-insert instead (apply the formatter's tier sort to this section),
    //       like `cargo add` does when the table is already sorted. Keep append when
    //       sort is off so the author's grouping is preserved.
    fn insert(&mut self, section: Section, package: &str, constraint: &str) {
        self.document
            .entry(section.as_str())
            .or_insert_with(toml_edit::table)[package] = toml_edit::value(constraint);
    }

    /// Remove `package` from `section`, dropping the section when it becomes empty.
    ///
    /// An empty `[require]` table is noise in a manifest, so removing the last entry
    /// removes the header with it.
    fn remove(&mut self, section: Section, package: &str) {
        let emptied = {
            let Some(table) = self
                .document
                .get_mut(section.as_str())
                .and_then(|item| item.as_table_mut())
            else {
                return;
            };

            table.remove(package);
            table.is_empty()
        };

        if emptied {
            self.document.remove(section.as_str());
        }
    }

    /// Declare `package` in `section` as inheriting the root's shared constraint.
    ///
    /// Written as `{ workspace = true }` rather than a copy of the root's constraint:
    /// a member that spells one of its own has to be edited again when the root moves,
    /// and the merge treats the two spellings as a conflict.
    fn inherit(&mut self, section: Section, package: &str) {
        let mut inherited = InlineTable::new();
        inherited.insert("workspace", true.into());
        inherited.fmt();

        self.document
            .entry(section.as_str())
            .or_insert_with(toml_edit::table)[package] = toml_edit::value(inherited);
    }
}
