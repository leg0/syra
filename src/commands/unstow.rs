use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::env::current_dir;
use std::fs::read_dir;
use std::path::{Path, PathBuf};

use crate::cli::UnstowArgs;
use crate::error::Error;
use crate::fs::{
    Action, BasePath, Package, PackageImpl, Symlink, TargetPath, execute_actions,
    is_owned_by_stow_directory, relative_path, resolve_link,
};

struct UnstowPlan {
    actions: Vec<Action>,
    removed: HashSet<PathBuf>,
    affected_roots: BTreeSet<PathBuf>,
}

pub fn run(args: &UnstowArgs) -> Result<(), Error> {
    if args.packages.is_empty() {
        eprintln!("error: At least one package is required");
        return Err(Error::MissingPackages);
    }

    let cwd = current_dir()?;
    let stow_dir = args.package_dir.as_ref().unwrap_or(&cwd).canonicalize()?;
    let target_dir = match &args.target_dir {
        Some(target_dir) => target_dir.canonicalize()?,
        None => stow_dir
            .parent()
            .ok_or(Error::DefaultTargetNotAvailable)?
            .canonicalize()?,
    };

    for pkg in &args.packages {
        if args.verbose {
            println!("Unstowing package: {}", pkg);
        }

        let package = PackageImpl::new(&stow_dir, pkg)?;
        let UnstowPlan {
            mut actions,
            removed,
            affected_roots,
        } = plan_unstow(&package, &target_dir, args.verbose)?;

        for root in affected_roots {
            if !removed.contains(&root) {
                let _ = plan_refold(&root, &stow_dir, &removed, args.verbose, &mut actions)?;
            }
        }

        execute_actions(&actions, args.simulate, args.verbose)?;

        if args.verbose {
            println!("Unstowed package: {}", pkg);
        }
    }

    Ok(())
}

fn plan_unstow<P: Package>(
    package: &P,
    target_dir: &Path,
    verbose: bool,
) -> Result<UnstowPlan, Error> {
    let mut actions = Vec::new();
    let mut removed = HashSet::new();
    let mut affected_roots = BTreeSet::new();

    for item in package.get_package_contents()? {
        let link_path = target_dir.join(&item);
        plan_unstow_item(
            &package.path().join(&item),
            &link_path,
            target_dir,
            verbose,
            &mut actions,
            &mut removed,
            &mut affected_roots,
        )?;
    }

    Ok(UnstowPlan {
        actions,
        removed,
        affected_roots,
    })
}

fn plan_unstow_item(
    package_path: &Path,
    link_path: &Path,
    target_dir: &Path,
    verbose: bool,
    actions: &mut Vec<Action>,
    removed: &mut HashSet<PathBuf>,
    affected_roots: &mut BTreeSet<PathBuf>,
) -> Result<(), Error> {
    if link_path.is_symlink() {
        if resolve_link(link_path)? == package_path.canonicalize()? {
            if verbose {
                println!("Scheduling symlink removal: {:?}", link_path);
            }
            actions.push(Action::RemoveSymlink(link_path.to_path_buf()));
            removed.insert(link_path.to_path_buf());
            if let Ok(relative) = link_path.strip_prefix(target_dir)
                && let Some(root) = relative.components().next()
            {
                affected_roots.insert(target_dir.join(root.as_os_str()));
            }
        }
        return Ok(());
    }

    if link_path.is_dir() && package_path.is_dir() {
        for entry in read_dir(package_path)? {
            let entry = entry?;
            plan_unstow_item(
                &entry.path(),
                &link_path.join(entry.file_name()),
                target_dir,
                verbose,
                actions,
                removed,
                affected_roots,
            )?;
        }
    }

    Ok(())
}

fn plan_refold(
    path: &Path,
    stow_dir: &Path,
    removed: &HashSet<PathBuf>,
    verbose: bool,
    actions: &mut Vec<Action>,
) -> Result<Option<PathBuf>, Error> {
    if path.is_symlink() || !path.is_dir() {
        return Ok(None);
    }

    let mut entries = BTreeMap::new();
    for entry in read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if removed.contains(&child) {
            continue;
        }

        let source = if child.is_symlink() {
            resolve_link(&child)?
        } else if child.is_dir() {
            let Some(source) = plan_refold(&child, stow_dir, removed, verbose, actions)? else {
                return Ok(None);
            };
            source
        } else {
            return Ok(None);
        };

        if !is_owned_by_stow_directory(&source, stow_dir) {
            return Ok(None);
        }
        entries.insert(entry.file_name(), (child, source));
    }

    if entries.is_empty() {
        return Ok(None);
    }

    let Some(source_dir) = entries
        .values()
        .next()
        .and_then(|(_, source)| source.parent())
        .map(Path::to_path_buf)
    else {
        return Ok(None);
    };

    if entries
        .values()
        .any(|(_, source)| source.parent() != Some(source_dir.as_path()))
    {
        return Ok(None);
    }

    let source_entries = read_dir(&source_dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let target_entries = entries.keys().cloned().collect::<BTreeSet<_>>();
    if source_entries != target_entries {
        return Ok(None);
    }

    for (name, (child, source)) in &entries {
        if source.file_name() != Some(name.as_os_str()) {
            return Ok(None);
        }
        actions.push(Action::RemoveSymlink(child.clone()));
    }
    actions.push(Action::RemoveDirectory(path.to_path_buf()));

    let parent = path.parent().ok_or(Error::DefaultTargetNotAvailable)?;
    let target = relative_path(TargetPath(&source_dir), BasePath(parent))?;
    if verbose {
        println!("Scheduling directory refold: {:?} -> {:?}", path, target);
    }
    actions.push(Action::CreateSymlink(Symlink {
        path: path.to_path_buf(),
        target,
    }));

    Ok(Some(source_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::fs::symlink;

    struct TestPackage {
        path: PathBuf,
    }

    impl Package for TestPackage {
        fn get_package_contents(&self) -> Result<Vec<PathBuf>, Error> {
            Ok(read_dir(&self.path)?
                .map(|entry| entry.map(|entry| PathBuf::from(entry.file_name())))
                .collect::<Result<_, _>>()?)
        }

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
            let root = std::env::temp_dir().join(format!("syra-unstow-{unique}"));
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
    fn removes_a_folded_package_link() {
        let tree = TestTree::new();
        let package_path = tree.root.join("package");
        let target_path = tree.root.join("target");
        create_dir_all(package_path.join("bin")).unwrap();
        create_dir_all(&target_path).unwrap();
        File::create(package_path.join("bin").join("tool.exe")).unwrap();
        let target = relative_path(
            TargetPath(&package_path.join("bin")),
            BasePath(&target_path),
        )
        .unwrap();
        symlink(target, target_path.join("bin")).unwrap();

        let package = TestPackage { path: package_path };
        let plan = plan_unstow(&package, &target_path, false).unwrap();
        execute_actions(&plan.actions, false, false).unwrap();

        assert!(!target_path.join("bin").exists());
        assert!(!target_path.join("bin").is_symlink());
    }

    #[test]
    fn refolds_directory_after_removing_another_package() {
        let tree = TestTree::new();
        let old_package = tree.root.join("old-package");
        let new_package = tree.root.join("new-package");
        let target_path = tree.root.join("target");
        create_dir_all(old_package.join("bin")).unwrap();
        create_dir_all(new_package.join("bin")).unwrap();
        create_dir_all(target_path.join("bin")).unwrap();
        File::create(old_package.join("bin").join("old.exe")).unwrap();
        File::create(new_package.join("bin").join("new.exe")).unwrap();
        let old_target = relative_path(
            TargetPath(&old_package.join("bin").join("old.exe")),
            BasePath(&target_path.join("bin")),
        )
        .unwrap();
        let new_target = relative_path(
            TargetPath(&new_package.join("bin").join("new.exe")),
            BasePath(&target_path.join("bin")),
        )
        .unwrap();
        symlink(old_target, target_path.join("bin").join("old.exe")).unwrap();
        symlink(new_target, target_path.join("bin").join("new.exe")).unwrap();

        let package = TestPackage { path: old_package };
        let UnstowPlan {
            mut actions,
            removed,
            affected_roots,
        } = plan_unstow(&package, &target_path, false).unwrap();
        for root in affected_roots {
            let _ = plan_refold(&root, &tree.root, &removed, false, &mut actions).unwrap();
        }
        execute_actions(&actions, false, false).unwrap();

        assert!(target_path.join("bin").is_symlink());
        assert_eq!(
            resolve_link(&target_path.join("bin")).unwrap(),
            new_package.join("bin").canonicalize().unwrap()
        );
    }
}
