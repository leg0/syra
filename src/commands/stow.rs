use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{read_dir, symlink_metadata};
use std::path::{Path, PathBuf};

use crate::cli::StowArgs;
use crate::error::Error;
use crate::fs::{Package, PackageImpl};
use crate::plan::{Entry, Plan};

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
    for pkg in &args.packages {
        if args.verbose {
            println!("Planning stow for package: {pkg}");
        }

        let package = PackageImpl::new(&stow_dir, pkg)?;
        for item in package.get_package_contents()? {
            plan = plan_item(
                plan,
                &package.path().join(&item),
                &target_dir.join(&item),
                pkg,
                args.verbose,
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
                plan = plan
                    .remove_symlink(link_path.to_path_buf())
                    .create_directory(link_path.to_path_buf());
                return plan_merged_directories(
                    plan,
                    &existing_path,
                    &package_path,
                    link_path,
                    pkg,
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

            for entry in read_dir(package_path)? {
                let entry = entry?;
                plan = plan_item(
                    plan,
                    &entry.path(),
                    &link_path.join(entry.file_name()),
                    pkg,
                    verbose,
                )?;
            }
            Ok(plan)
        }
        Entry::File => Err(Error::LinkPathExists(link_path.to_path_buf())),
        Entry::Missing => plan.create_symlink(link_path.to_path_buf(), package_path),
    }
}

fn plan_merged_directories(
    mut plan: Plan,
    existing: &Path,
    package: &Path,
    link_path: &Path,
    pkg: &str,
) -> Result<Plan, Error> {
    let existing_entries = directory_entries(existing)?;
    let package_entries = directory_entries(package)?;
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
            (Some(existing_item), None) => plan.create_symlink(target_item, existing_item)?,
            (None, Some(package_item)) => plan.create_symlink(target_item, package_item)?,
            (Some(existing_item), Some(package_item)) => {
                let existing_is_dir = symlink_metadata(existing_item)?.file_type().is_dir();
                let package_is_dir = symlink_metadata(package_item)?.file_type().is_dir();
                if existing_is_dir && package_is_dir {
                    let plan = plan.create_directory(target_item.clone());
                    plan_merged_directories(plan, existing_item, package_item, &target_item, pkg)?
                } else if existing_item.canonicalize()? == package_item.canonicalize()? {
                    plan.create_symlink(target_item, package_item)?
                } else {
                    return Err(Error::LinkNotOwnedByPackage(target_item, pkg.to_string()));
                }
            }
            (None, None) => unreachable!(),
        };
    }
    Ok(plan)
}

fn directory_entries(path: &Path) -> Result<BTreeMap<OsString, PathBuf>, Error> {
    read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            Ok((entry.file_name(), entry.path()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all, remove_dir_all};
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
