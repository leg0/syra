use std::collections::BTreeSet;
use std::fs::symlink_metadata;
use std::path::Path;

use crate::cli::StowArgs;
use crate::error::Error;
use crate::fs::PackageImpl;
use crate::ignore::{IgnoreContext, PackageIgnore};
use crate::plan::{Entry, Plan};

struct MergeContext<'a> {
    package_name: &'a str,
    verbose: bool,
    ignore_context: &'a IgnoreContext,
    existing_ignore: &'a PackageIgnore,
    package_ignore: &'a PackageIgnore,
}

pub fn run(args: &StowArgs) -> Result<(), Error> {
    let plan = plan(Plan::from_args(args)?, args)?;
    plan.execute(args.simulate, args.verbose)
}

pub fn plan(mut plan: Plan, args: &StowArgs) -> Result<Plan, Error> {
    if args.packages.is_empty() {
        return Err(Error::MissingPackages);
    }
    if args.verbose {
        println!(
            "Stowing packages {:?}, src={:?}, dst={:?}",
            args.packages, args.package_dir, args.target_dir
        );
    }

    let stow_dir = plan.stow_dir().to_path_buf();
    let target_dir = plan.target_dir().to_path_buf();
    let ignore_context = IgnoreContext::system()?;
    for pkg in &args.packages {
        if args.verbose {
            println!("Planning stow for package: {pkg}");
        }

        let package = PackageImpl::new(&stow_dir, pkg)?;
        let package_ignore = ignore_context.for_package(package.path())?;
        for (_, package_path) in package_ignore.read_directory(package.path())? {
            let item = package_path
                .file_name()
                .ok_or_else(|| Error::PathOutsidePackage(package_path.clone()))?;
            plan = plan_item(
                plan,
                &package_path,
                &target_dir.join(item),
                pkg,
                args.verbose,
                &ignore_context,
                &package_ignore,
            )?;
        }
    }

    Ok(plan)
}

fn plan_item(
    mut plan: Plan,
    package_path: &Path,
    link_path: &Path,
    pkg: &str,
    verbose: bool,
    ignore_context: &IgnoreContext,
    package_ignore: &PackageIgnore,
) -> Result<Plan, Error> {
    if verbose {
        println!(
            "Planning stow item: {} -> {}",
            link_path.display(),
            package_path.display()
        );
    }

    match plan.entry(link_path)? {
        Entry::Symlink(existing_path) => {
            let package_path = package_path.canonicalize()?;
            if existing_path == package_path {
                return Ok(plan);
            }

            if package_path.is_dir()
                && existing_path.is_dir()
                && plan.is_owned_source(&existing_path)
            {
                let existing_ignore = ignore_context.for_source(plan.stow_dir(), &existing_path)?;
                plan = plan
                    .remove_symlink(link_path.to_path_buf())
                    .create_directory(link_path.to_path_buf());
                return plan_merged_directories(
                    plan,
                    &existing_path,
                    &package_path,
                    link_path,
                    &MergeContext {
                        package_name: pkg,
                        verbose,
                        ignore_context,
                        existing_ignore: &existing_ignore,
                        package_ignore,
                    },
                );
            }

            Err(Error::LinkNotOwnedByPackage(
                link_path.to_path_buf(),
                pkg.to_string(),
            ))
        }
        Entry::Directory => {
            if !symlink_metadata(package_path)?.file_type().is_dir() {
                return Err(Error::LinkPathExists(link_path.to_path_buf()));
            }

            for (name, entry_path) in package_ignore.read_directory(package_path)? {
                plan = plan_item(
                    plan,
                    &entry_path,
                    &link_path.join(name),
                    pkg,
                    verbose,
                    ignore_context,
                    package_ignore,
                )?;
            }
            Ok(plan)
        }
        Entry::File => Err(Error::LinkPathExists(link_path.to_path_buf())),
        Entry::Missing => {
            if symlink_metadata(package_path)?.file_type().is_dir()
                && package_ignore.contains_ignored_entries(package_path)?
            {
                plan = plan.create_directory(link_path.to_path_buf());
                for (name, entry_path) in package_ignore.read_directory(package_path)? {
                    plan = plan_item(
                        plan,
                        &entry_path,
                        &link_path.join(name),
                        pkg,
                        verbose,
                        ignore_context,
                        package_ignore,
                    )?;
                }
                Ok(plan)
            } else {
                plan.create_symlink(link_path.to_path_buf(), package_path)
            }
        }
    }
}

fn plan_merged_directories(
    mut plan: Plan,
    existing: &Path,
    package: &Path,
    link_path: &Path,
    context: &MergeContext,
) -> Result<Plan, Error> {
    let existing_entries = context.existing_ignore.read_directory(existing)?;
    let package_entries = context.package_ignore.read_directory(package)?;
    let names: BTreeSet<_> = existing_entries
        .keys()
        .chain(package_entries.keys())
        .cloned()
        .collect();

    for name in names {
        let existing_item = existing_entries.get(&name);
        let package_item = package_entries.get(&name);
        let target_item = link_path.join(&name);

        plan = match (existing_item, package_item) {
            (Some(existing_item), None) => plan_item(
                plan,
                existing_item,
                &target_item,
                context.package_name,
                context.verbose,
                context.ignore_context,
                context.existing_ignore,
            )?,
            (None, Some(package_item)) => plan_item(
                plan,
                package_item,
                &target_item,
                context.package_name,
                context.verbose,
                context.ignore_context,
                context.package_ignore,
            )?,
            (Some(existing_item), Some(package_item)) => {
                let existing_is_dir = symlink_metadata(existing_item)?.file_type().is_dir();
                let package_is_dir = symlink_metadata(package_item)?.file_type().is_dir();
                if existing_is_dir && package_is_dir {
                    let plan = plan.create_directory(target_item.clone());
                    plan_merged_directories(
                        plan,
                        existing_item,
                        package_item,
                        &target_item,
                        context,
                    )?
                } else if existing_item.canonicalize()? == package_item.canonicalize()? {
                    plan.create_symlink(target_item, package_item)?
                } else {
                    return Err(Error::LinkNotOwnedByPackage(
                        target_item,
                        context.package_name.to_string(),
                    ));
                }
            }
            (None, None) => unreachable!(),
        };
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all, remove_dir_all};
    use std::path::PathBuf;
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
            let root = std::env::temp_dir().join(format!("syra-stow-{unique}"));
            create_dir_all(root.join("stow")).unwrap();
            Self { root }
        }

        fn args(&self, package: &str) -> StowArgs {
            StowArgs {
                package_dir: Some(self.root.join("stow")),
                target_dir: Some(self.root.clone()),
                packages: vec![package.to_string()],
                verbose: false,
                simulate: false,
            }
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn folds_package_directory_into_existing_target_directory() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("stow").join("package").join("bin")).unwrap();
        create_dir_all(tree.root.join("bin")).unwrap();
        File::create(
            tree.root
                .join("stow")
                .join("package")
                .join("bin")
                .join("tool.exe"),
        )
        .unwrap();

        run(&tree.args("package")).unwrap();

        assert!(tree.root.join("bin").join("tool.exe").is_symlink());
    }

    #[test]
    fn repeated_stow_is_a_no_op() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("stow").join("package")).unwrap();
        File::create(tree.root.join("stow").join("package").join("file.txt")).unwrap();

        run(&tree.args("package")).unwrap();
        run(&tree.args("package")).unwrap();

        assert!(tree.root.join("file.txt").is_symlink());
    }

    #[test]
    fn unfolds_a_directory_link_owned_by_another_package() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("stow").join("old-package").join("bin")).unwrap();
        create_dir_all(tree.root.join("stow").join("new-package").join("bin")).unwrap();
        File::create(
            tree.root
                .join("stow")
                .join("old-package")
                .join("bin")
                .join("old.exe"),
        )
        .unwrap();
        File::create(
            tree.root
                .join("stow")
                .join("new-package")
                .join("bin")
                .join("new.exe"),
        )
        .unwrap();

        run(&tree.args("old-package")).unwrap();
        run(&tree.args("new-package")).unwrap();

        assert!(tree.root.join("bin").is_dir());
        assert!(!tree.root.join("bin").is_symlink());
        assert!(tree.root.join("bin").join("old.exe").is_symlink());
        assert!(tree.root.join("bin").join("new.exe").is_symlink());
    }

    #[test]
    fn ignored_entries_are_not_exposed_through_directory_folding() {
        let tree = TestTree::new();
        let package = tree.root.join("stow").join("package");
        create_dir_all(package.join("bin")).unwrap();
        create_dir_all(package.join("cache")).unwrap();
        File::create(package.join("bin").join("keep.exe")).unwrap();
        File::create(package.join("bin").join("ignored.tmp")).unwrap();
        File::create(package.join("cache").join("secret.txt")).unwrap();
        std::fs::write(package.join(".stow-local-ignore"), ".*\\.tmp\ncache\n").unwrap();

        run(&tree.args("package")).unwrap();

        assert!(tree.root.join("bin").is_dir());
        assert!(!tree.root.join("bin").is_symlink());
        assert!(tree.root.join("bin").join("keep.exe").is_symlink());
        assert!(!tree.root.join("bin").join("ignored.tmp").exists());
        assert!(!tree.root.join("cache").exists());
        assert!(!tree.root.join(".stow-local-ignore").exists());
    }

    #[test]
    fn packages_in_one_plan_use_independent_ignore_rules() {
        let tree = TestTree::new();
        let first = tree.root.join("stow").join("first");
        let second = tree.root.join("stow").join("second");
        create_dir_all(&first).unwrap();
        create_dir_all(&second).unwrap();
        File::create(first.join("first.keep")).unwrap();
        File::create(first.join("first.skip")).unwrap();
        File::create(second.join("second.keep")).unwrap();
        File::create(second.join("second.skip")).unwrap();
        std::fs::write(first.join(".stow-local-ignore"), "first\\.skip\n").unwrap();
        std::fs::write(second.join(".stow-local-ignore"), "second\\.skip\n").unwrap();
        let mut args = tree.args("first");
        args.packages.push("second".to_string());

        run(&args).unwrap();

        assert!(tree.root.join("first.keep").is_symlink());
        assert!(!tree.root.join("first.skip").exists());
        assert!(tree.root.join("second.keep").is_symlink());
        assert!(!tree.root.join("second.skip").exists());
    }

    #[test]
    fn unfolding_applies_each_source_packages_current_ignore_rules() {
        let tree = TestTree::new();
        let old_package = tree.root.join("stow").join("old-package");
        let new_package = tree.root.join("stow").join("new-package");
        create_dir_all(old_package.join("bin")).unwrap();
        create_dir_all(new_package.join("bin")).unwrap();
        File::create(old_package.join("bin").join("old.exe")).unwrap();

        run(&tree.args("old-package")).unwrap();
        assert!(tree.root.join("bin").is_symlink());

        File::create(old_package.join("bin").join("old.tmp")).unwrap();
        std::fs::write(old_package.join(".stow-local-ignore"), ".*\\.tmp\n").unwrap();
        File::create(new_package.join("bin").join("new.exe")).unwrap();
        File::create(new_package.join("bin").join("new.tmp")).unwrap();
        std::fs::write(new_package.join(".stow-local-ignore"), ".*\\.tmp\n").unwrap();

        run(&tree.args("new-package")).unwrap();

        assert!(tree.root.join("bin").is_dir());
        assert!(tree.root.join("bin").join("old.exe").is_symlink());
        assert!(tree.root.join("bin").join("new.exe").is_symlink());
        assert!(!tree.root.join("bin").join("old.tmp").exists());
        assert!(!tree.root.join("bin").join("new.tmp").exists());
    }

    #[test]
    fn conflict_in_later_package_does_not_install_earlier_package() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("stow").join("first")).unwrap();
        create_dir_all(tree.root.join("stow").join("second")).unwrap();
        File::create(tree.root.join("stow").join("first").join("first.txt")).unwrap();
        File::create(tree.root.join("stow").join("second").join("conflict.txt")).unwrap();
        File::create(tree.root.join("conflict.txt")).unwrap();
        let mut args = tree.args("first");
        args.packages.push("second".to_string());

        assert!(run(&args).is_err());
        assert!(!tree.root.join("first.txt").exists());
        assert!(!tree.root.join("first.txt").is_symlink());
    }
}
