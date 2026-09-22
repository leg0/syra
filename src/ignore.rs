use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs::{read_dir, read_to_string};
use std::path::{Component, Path, PathBuf};

use regex::Regex;

use crate::error::Error;

const LOCAL_IGNORE_FILE: &str = ".stow-local-ignore";
const GLOBAL_IGNORE_FILE: &str = ".stow-global-ignore";
const BUILT_IN_IGNORE_LIST: &str = r#"
RCS
.+,v

CVS
\.\#.+
\.cvsignore

\.svn
_darcs
\.hg

\.git
\.gitignore
\.gitmodules

.+~
\#.*\#

^/README.*
^/LICENSE.*
^/COPYING
"#;

#[derive(Clone)]
struct RuleSet {
    basename: Vec<Regex>,
    path: Vec<Regex>,
}

pub struct IgnoreContext {
    fallback: RuleSet,
}

pub struct PackageIgnore {
    package_root: PathBuf,
    rules: RuleSet,
}

impl IgnoreContext {
    pub fn system() -> Result<Self, Error> {
        Self::with_home(home_dir().as_deref())
    }

    fn with_home(home: Option<&Path>) -> Result<Self, Error> {
        let global_path = home.map(|home| home.join(GLOBAL_IGNORE_FILE));
        let fallback = match global_path {
            Some(path) if path.exists() => RuleSet::from_file(&path)?,
            _ => RuleSet::parse(
                Path::new("<GNU Stow built-in ignore list>"),
                BUILT_IN_IGNORE_LIST,
            )?,
        };
        Ok(Self { fallback })
    }

    pub fn for_package(&self, package_root: &Path) -> Result<PackageIgnore, Error> {
        if !package_root.is_absolute() {
            return Err(Error::PathNotAbsolute);
        }
        let package_root = package_root.to_path_buf();
        let local_path = package_root.join(LOCAL_IGNORE_FILE);
        let rules = if local_path.exists() {
            RuleSet::from_file(&local_path)?
        } else {
            self.fallback.clone()
        };
        Ok(PackageIgnore {
            package_root,
            rules,
        })
    }

    pub fn for_source(&self, stow_dir: &Path, source: &Path) -> Result<PackageIgnore, Error> {
        let stow_dir = stow_dir.canonicalize()?;
        let source = source.canonicalize()?;
        let relative = source
            .strip_prefix(&stow_dir)
            .map_err(|_| Error::PathOutsidePackage(source.clone()))?;
        let package_name = relative
            .components()
            .next()
            .and_then(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .ok_or_else(|| Error::PathOutsidePackage(source.clone()))?;
        self.for_package(&stow_dir.join(package_name))
    }
}

impl PackageIgnore {
    pub fn read_directory(&self, directory: &Path) -> Result<BTreeMap<OsString, PathBuf>, Error> {
        let mut entries = BTreeMap::new();
        for entry in read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if !self.is_ignored(&path)? {
                entries.insert(entry.file_name(), path);
            }
        }
        Ok(entries)
    }

    pub fn contains_ignored_entries(&self, directory: &Path) -> Result<bool, Error> {
        for entry in read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if self.is_ignored(&path)? {
                return Ok(true);
            }
            if entry.file_type()?.is_dir() && self.contains_ignored_entries(&path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn is_ignored(&self, path: &Path) -> Result<bool, Error> {
        let relative = path
            .strip_prefix(&self.package_root)
            .map_err(|_| Error::PathOutsidePackage(path.to_path_buf()))?;
        let components = relative
            .components()
            .map(|component| match component {
                Component::Normal(value) => value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::NonUnicodePath(path.to_path_buf())),
                _ => Err(Error::PathOutsidePackage(path.to_path_buf())),
            })
            .collect::<Result<Vec<_>, _>>()?;

        if components.len() == 1 && components[0] == LOCAL_IGNORE_FILE {
            return Ok(true);
        }

        let Some(basename) = components.last() else {
            return Ok(false);
        };
        if self
            .rules
            .basename
            .iter()
            .any(|rule| rule.is_match(basename))
        {
            return Ok(true);
        }

        let relative_path = format!("/{}", components.join("/"));
        Ok(self
            .rules
            .path
            .iter()
            .any(|rule| rule.is_match(&relative_path)))
    }
}

impl RuleSet {
    fn from_file(path: &Path) -> Result<Self, Error> {
        Self::parse(path, &read_to_string(path)?)
    }

    fn parse(path: &Path, contents: &str) -> Result<Self, Error> {
        let mut basename = Vec::new();
        let mut path_rules = Vec::new();

        for (index, line) in contents.lines().enumerate() {
            let pattern = strip_comment(line).trim();
            if pattern.is_empty() {
                continue;
            }

            let expression = if pattern.contains('/') {
                format!(r"(?:\A|/)(?:{pattern})\z")
            } else {
                format!(r"\A(?:{pattern})\z")
            };
            let rule = Regex::new(&expression).map_err(|error| Error::InvalidIgnorePattern {
                path: path.to_path_buf(),
                line: index + 1,
                pattern: pattern.to_string(),
                message: error.to_string(),
            })?;

            if pattern.contains('/') {
                path_rules.push(rule);
            } else {
                basename.push(rule);
            }
        }

        Ok(Self {
            basename,
            path: path_rules,
        })
    }
}

fn strip_comment(line: &str) -> &str {
    for (index, character) in line.char_indices() {
        if character != '#' {
            continue;
        }
        let preceding_backslashes = line[..index]
            .chars()
            .rev()
            .take_while(|character| *character == '\\')
            .count();
        if preceding_backslashes % 2 == 0 {
            return &line[..index];
        }
    }
    line
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            let drive = env::var_os("HOMEDRIVE")?;
            let path = env::var_os("HOMEPATH")?;
            Some(PathBuf::from(drive).join(path))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all, remove_dir_all, write};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestTree {
        root: PathBuf,
    }

    impl TestTree {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = env::temp_dir().join(format!("syra-ignore-{unique}"));
            create_dir_all(root.join("home")).unwrap();
            create_dir_all(root.join("package")).unwrap();
            Self { root }
        }

        fn context(&self) -> IgnoreContext {
            IgnoreContext::with_home(Some(&self.root.join("home"))).unwrap()
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn matches_basename_and_suffix_subpath_rules_exactly() {
        let tree = TestTree::new();
        write(
            tree.root.join("package").join(LOCAL_IGNORE_FILE),
            "cache\nlogs/.*\\.txt\n^/root/only$\n",
        )
        .unwrap();
        create_dir_all(tree.root.join("package").join("foo").join("logs")).unwrap();
        create_dir_all(tree.root.join("package").join("root")).unwrap();
        File::create(tree.root.join("package").join("cache")).unwrap();
        File::create(tree.root.join("package").join("cache-old")).unwrap();
        File::create(
            tree.root
                .join("package")
                .join("foo")
                .join("logs")
                .join("trace.txt"),
        )
        .unwrap();
        File::create(tree.root.join("package").join("root").join("only")).unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("cache"))
                .unwrap()
        );
        assert!(
            !rules
                .is_ignored(&tree.root.join("package").join("cache-old"))
                .unwrap()
        );
        assert!(
            rules
                .is_ignored(
                    &tree
                        .root
                        .join("package")
                        .join("foo")
                        .join("logs")
                        .join("trace.txt")
                )
                .unwrap()
        );
        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("root").join("only"))
                .unwrap()
        );
    }

    #[test]
    fn parses_comments_escaped_hashes_and_blank_lines() {
        let tree = TestTree::new();
        write(
            tree.root.join("package").join(LOCAL_IGNORE_FILE),
            "\n  # comment\n\\#literal # trailing comment\n",
        )
        .unwrap();
        File::create(tree.root.join("package").join("#literal")).unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("#literal"))
                .unwrap()
        );
    }

    #[test]
    fn local_rules_replace_global_rules() {
        let tree = TestTree::new();
        write(tree.root.join("home").join(GLOBAL_IGNORE_FILE), "global\n").unwrap();
        write(tree.root.join("package").join(LOCAL_IGNORE_FILE), "local\n").unwrap();
        File::create(tree.root.join("package").join("global")).unwrap();
        File::create(tree.root.join("package").join("local")).unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            !rules
                .is_ignored(&tree.root.join("package").join("global"))
                .unwrap()
        );
        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("local"))
                .unwrap()
        );
    }

    #[test]
    fn global_rules_replace_built_in_rules() {
        let tree = TestTree::new();
        write(tree.root.join("home").join(GLOBAL_IGNORE_FILE), "global\n").unwrap();
        File::create(tree.root.join("package").join(".git")).unwrap();
        File::create(tree.root.join("package").join("global")).unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            !rules
                .is_ignored(&tree.root.join("package").join(".git"))
                .unwrap()
        );
        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("global"))
                .unwrap()
        );
    }

    #[test]
    fn built_in_rules_ignore_repository_metadata_and_root_docs() {
        let tree = TestTree::new();
        File::create(tree.root.join("package").join(".git")).unwrap();
        File::create(tree.root.join("package").join("README.md")).unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            rules
                .is_ignored(&tree.root.join("package").join(".git"))
                .unwrap()
        );
        assert!(
            rules
                .is_ignored(&tree.root.join("package").join("README.md"))
                .unwrap()
        );
    }

    #[test]
    fn only_top_level_local_ignore_file_is_special() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("package").join("nested")).unwrap();
        File::create(tree.root.join("package").join(LOCAL_IGNORE_FILE)).unwrap();
        File::create(
            tree.root
                .join("package")
                .join("nested")
                .join(LOCAL_IGNORE_FILE),
        )
        .unwrap();

        let rules = tree
            .context()
            .for_package(&tree.root.join("package"))
            .unwrap();

        assert!(
            rules
                .is_ignored(&tree.root.join("package").join(LOCAL_IGNORE_FILE))
                .unwrap()
        );
        assert!(
            !rules
                .is_ignored(
                    &tree
                        .root
                        .join("package")
                        .join("nested")
                        .join(LOCAL_IGNORE_FILE)
                )
                .unwrap()
        );
    }

    #[test]
    fn invalid_regex_reports_source_line() {
        let tree = TestTree::new();
        let path = tree.root.join("package").join(LOCAL_IGNORE_FILE);
        write(&path, "valid\n(?=unsupported)\n").unwrap();

        let error = tree
            .context()
            .for_package(&tree.root.join("package"))
            .err()
            .unwrap();

        match error {
            Error::InvalidIgnorePattern {
                path: actual_path,
                line,
                pattern,
                ..
            } => {
                assert_eq!(actual_path, path);
                assert_eq!(line, 2);
                assert_eq!(pattern, "(?=unsupported)");
            }
            _ => panic!("expected InvalidIgnorePattern"),
        }
    }
}
