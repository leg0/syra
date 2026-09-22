use std::collections::{BTreeMap, BTreeSet};
use std::env::current_dir;
use std::fs::{read_dir, symlink_metadata};
use std::path::Path;

use crate::cli;
use crate::error::Error;
use crate::fs::{
    Action, BasePath, Package, PackageImpl, Symlink, Target, TargetImpl, TargetPath,
    execute_actions, is_owned_by_stow_directory, relative_path, resolve_link,
};

pub fn run(args: &cli::StowArgs) -> Result<(), Error> {
    if args.packages.is_empty() {
        eprintln!("error: At least one package is required");
        return Err(Error::MissingPackages);
    }
    if args.verbose {
        println!(
            "Stowing packages {:?}, src={:?}, dst={:?}",
            args.packages, args.package_dir, args.target_dir
        );
    }

    let cwd = current_dir()?;
    let package_dir = args.package_dir.as_ref().unwrap_or(&cwd).canonicalize()?;

    let target_dir = match &args.target_dir {
        Some(target_dir) => target_dir.canonicalize()?,
        None => package_dir
            .parent()
            .ok_or(Error::DefaultTargetNotAvailable)?
            .canonicalize()?,
    };

    let target = TargetImpl::new(&target_dir)?;
    for pkg in args.packages.iter() {
        if args.verbose {
            println!("Stowing package: {}", pkg);
        }

        let package = PackageImpl::new(&package_dir, pkg)?;
        if args.verbose {
            println!("Package path: {:?}", package.path());
        }
        let actions = do_stow(&package, &target, &package_dir, pkg, args.verbose)?;
        execute_actions(&actions, args.simulate, args.verbose)?;

        if args.verbose {
            println!("Stowed package: {}", pkg);
        }
    }

    Ok(())
}

fn do_stow<P: Package, T: Target>(
    package: &P,
    target: &T,
    stow_dir: &Path,
    pkg: &str,
    verbose: bool,
) -> Result<Vec<Action>, Error> {
    let package_path = package.path();
    let target_dir = target.path();
    let mut actions = Vec::new();

    for item in package.get_package_contents()? {
        plan_item(
            &package_path.join(&item),
            &target_dir.join(&item),
            stow_dir,
            pkg,
            verbose,
            &mut actions,
        )?;
    }

    Ok(actions)
}

fn plan_item(
    package_path: &Path,
    link_path: &Path,
    stow_dir: &Path,
    pkg: &str,
    verbose: bool,
    actions: &mut Vec<Action>,
) -> Result<(), Error> {
    let link_parent = link_path.parent().ok_or(Error::DefaultTargetNotAvailable)?;
    let link_target = relative_path(TargetPath(package_path), BasePath(link_parent))?;

    if verbose {
        println!(
            "stow::run: Stowing item: {}, link_path={}",
            package_path.display(),
            link_path.display()
        );
    }

    if link_path.is_symlink() {
        let existing_path = resolve_link(link_path)?;
        let package_path = package_path.canonicalize()?;
        if existing_path == package_path {
            if verbose {
                println!(
                    "symlink({:?}, {:?}) already exists and points to the same target",
                    link_path, link_target
                );
            }
            return Ok(());
        }

        if package_path.is_dir()
            && existing_path.is_dir()
            && is_owned_by_stow_directory(&existing_path, stow_dir)
        {
            actions.push(Action::RemoveSymlink(link_path.to_path_buf()));
            actions.push(Action::CreateDirectory(link_path.to_path_buf()));
            plan_merged_directories(
                &existing_path,
                &package_path,
                link_path,
                pkg,
                verbose,
                actions,
            )?;
            return Ok(());
        }

        return Err(Error::LinkNotOwnedByPackage(
            link_path.to_path_buf(),
            pkg.to_string(),
        ));
    }

    if link_path.exists() {
        let package_is_directory = symlink_metadata(package_path)?.file_type().is_dir();
        if link_path.is_dir() && package_is_directory {
            for entry in read_dir(package_path)? {
                let entry = entry?;
                plan_item(
                    &entry.path(),
                    &link_path.join(entry.file_name()),
                    stow_dir,
                    pkg,
                    verbose,
                    actions,
                )?;
            }
            return Ok(());
        }

        return Err(Error::LinkPathExists(link_path.to_path_buf()));
    }

    if verbose {
        println!(
            "stow::run: Scheduling symlink creation: {:?} -> {:?}",
            link_path, link_target
        );
    }
    actions.push(Action::CreateSymlink(Symlink {
        path: link_path.to_path_buf(),
        target: link_target,
    }));

    Ok(())
}

fn plan_merged_directories(
    existing: &Path,
    package: &Path,
    link_path: &Path,
    pkg: &str,
    verbose: bool,
    actions: &mut Vec<Action>,
) -> Result<(), Error> {
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

        match (existing_item, package_item) {
            (Some(existing_item), None) => {
                schedule_symlink(existing_item, &target_item, verbose, actions)?;
            }
            (None, Some(package_item)) => {
                schedule_symlink(package_item, &target_item, verbose, actions)?;
            }
            (Some(existing_item), Some(package_item)) => {
                let existing_is_dir = symlink_metadata(existing_item)?.file_type().is_dir();
                let package_is_dir = symlink_metadata(package_item)?.file_type().is_dir();
                if existing_is_dir && package_is_dir {
                    actions.push(Action::CreateDirectory(target_item.clone()));
                    plan_merged_directories(
                        existing_item,
                        package_item,
                        &target_item,
                        pkg,
                        verbose,
                        actions,
                    )?;
                } else if existing_item.canonicalize()? == package_item.canonicalize()? {
                    schedule_symlink(package_item, &target_item, verbose, actions)?;
                } else {
                    return Err(Error::LinkNotOwnedByPackage(target_item, pkg.to_string()));
                }
            }
            (None, None) => unreachable!(),
        }
    }

    Ok(())
}

fn directory_entries(
    path: &Path,
) -> Result<BTreeMap<std::ffi::OsString, std::path::PathBuf>, Error> {
    read_dir(path)?
        .map(|entry| {
            let entry = entry?;
            Ok((entry.file_name(), entry.path()))
        })
        .collect()
}

fn schedule_symlink(
    package_path: &Path,
    link_path: &Path,
    verbose: bool,
    actions: &mut Vec<Action>,
) -> Result<(), Error> {
    let link_parent = link_path.parent().ok_or(Error::DefaultTargetNotAvailable)?;
    let link_target = relative_path(TargetPath(package_path), BasePath(link_parent))?;
    if verbose {
        println!(
            "stow::run: Scheduling symlink creation: {:?} -> {:?}",
            link_path, link_target
        );
    }
    actions.push(Action::CreateSymlink(Symlink {
        path: link_path.to_path_buf(),
        target: link_target,
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::fs::{Package, Target, symlink};

    struct TestPackage {
        path: PathBuf,
    }
    impl Package for TestPackage {
        fn path(&self) -> &Path {
            &self.path
        }

        fn get_package_contents(&self) -> Result<Vec<PathBuf>, Error> {
            Ok(read_dir(&self.path)?
                .map(|entry| entry.map(|entry| PathBuf::from(entry.file_name())))
                .collect::<Result<_, _>>()?)
        }
    }

    struct TestTarget {
        path: PathBuf,
    }
    impl Target for TestTarget {
        fn path(&self) -> &Path {
            &self.path
        }
    }

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
            create_dir_all(&root).unwrap();
            Self { root }
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn folds_package_directory_into_existing_target_directory() {
        let tree = TestTree::new();
        let package_path = tree.root.join("nvim-win64-0.12.5");
        let target_path = tree.root.join("target");
        create_dir_all(package_path.join("bin")).unwrap();
        create_dir_all(target_path.join("bin")).unwrap();
        File::create(package_path.join("bin").join("nvim.exe")).unwrap();

        let package = TestPackage {
            path: package_path.clone(),
        };
        let target = TestTarget {
            path: target_path.clone(),
        };
        let actions = do_stow(&package, &target, &tree.root, ".", false).unwrap();

        assert_eq!(actions.len(), 1);
        let Action::CreateSymlink(action) = &actions[0] else {
            panic!("expected a symlink action");
        };
        assert_eq!(action.path, target_path.join("bin").join("nvim.exe"));
        assert_eq!(
            action.target,
            relative_path(
                TargetPath(&package_path.join("bin").join("nvim.exe")),
                BasePath(&target_path.join("bin"))
            )
            .unwrap()
        );
    }

    #[test]
    fn does_not_recreate_an_existing_owned_link() {
        let tree = TestTree::new();
        let package_path = tree.root.join("package");
        let target_path = tree.root.join("target");
        create_dir_all(&package_path).unwrap();
        create_dir_all(&target_path).unwrap();
        File::create(package_path.join("file.txt")).unwrap();
        let link_target = relative_path(
            TargetPath(&package_path.join("file.txt")),
            BasePath(&target_path),
        )
        .unwrap();
        symlink(&link_target, target_path.join("file.txt")).unwrap();

        let package = TestPackage { path: package_path };
        let target = TestTarget { path: target_path };
        let actions = do_stow(&package, &target, &tree.root, "package", false).unwrap();

        assert!(actions.is_empty());
    }

    #[test]
    fn unfolds_a_directory_link_owned_by_another_package() {
        let tree = TestTree::new();
        let old_package = tree.root.join("old-package");
        let new_package = tree.root.join("new-package");
        let target_path = tree.root.join("target");
        create_dir_all(old_package.join("bin")).unwrap();
        create_dir_all(new_package.join("bin")).unwrap();
        create_dir_all(&target_path).unwrap();
        File::create(old_package.join("bin").join("old.exe")).unwrap();
        File::create(new_package.join("bin").join("new.exe")).unwrap();
        let old_target =
            relative_path(TargetPath(&old_package.join("bin")), BasePath(&target_path)).unwrap();
        symlink(&old_target, target_path.join("bin")).unwrap();

        let package = TestPackage { path: new_package };
        let target = TestTarget {
            path: target_path.clone(),
        };
        let actions = do_stow(&package, &target, &tree.root, "new-package", false).unwrap();
        execute_actions(&actions, false, false).unwrap();

        assert!(target_path.join("bin").is_dir());
        assert!(target_path.join("bin").join("old.exe").is_symlink());
        assert!(target_path.join("bin").join("new.exe").is_symlink());
    }
}
