use std::collections::{BTreeMap, BTreeSet};
use std::fs::read_dir;
use std::path::{Path, PathBuf};

use crate::cli::UnstowArgs;
use crate::error::Error;
use crate::fs::{Package, PackageImpl};
use crate::plan::{Entry, Plan};

pub fn run(args: &UnstowArgs) -> Result<(), Error> {
    let plan = plan(Plan::from_args(args)?, args)?;
    plan.execute(args.simulate, args.verbose)
}

pub fn plan(mut plan: Plan, args: &UnstowArgs) -> Result<Plan, Error> {
    if args.packages.is_empty() {
        return Err(Error::MissingPackages);
    }

    let stow_dir = plan.stow_dir().to_path_buf();
    let target_dir = plan.target_dir().to_path_buf();
    for pkg in &args.packages {
        if args.verbose {
            println!("Planning unstow for package: {pkg}");
        }

        let package = PackageImpl::new(&stow_dir, pkg)?;
        let mut affected_roots = BTreeSet::new();
        for item in package.get_package_contents()? {
            plan = plan_unstow_item(
                plan,
                &package.path().join(&item),
                &target_dir.join(&item),
                &target_dir,
                &mut affected_roots,
                args.verbose,
            )?;
        }

        for root in affected_roots {
            if !matches!(plan.entry(&root)?, Entry::Missing) {
                (plan, _) = plan_refold(plan, &root, args.verbose)?;
            }
        }
    }

    Ok(plan)
}

fn plan_unstow_item(
    mut plan: Plan,
    package_path: &Path,
    link_path: &Path,
    target_dir: &Path,
    affected_roots: &mut BTreeSet<PathBuf>,
    verbose: bool,
) -> Result<Plan, Error> {
    match plan.entry(link_path)? {
        Entry::Symlink(source) => {
            if source == package_path.canonicalize()? {
                if verbose {
                    println!("Planning symlink removal: {:?}", link_path);
                }
                plan = plan.remove_symlink(link_path.to_path_buf());
                if let Ok(relative) = link_path.strip_prefix(target_dir)
                    && let Some(root) = relative.components().next()
                {
                    affected_roots.insert(target_dir.join(root.as_os_str()));
                }
            }
        }
        Entry::Directory if package_path.is_dir() => {
            for entry in read_dir(package_path)? {
                let entry = entry?;
                plan = plan_unstow_item(
                    plan,
                    &entry.path(),
                    &link_path.join(entry.file_name()),
                    target_dir,
                    affected_roots,
                    verbose,
                )?;
            }
        }
        Entry::Missing | Entry::File | Entry::Directory => {}
    }

    Ok(plan)
}

fn plan_refold(
    mut plan: Plan,
    path: &Path,
    verbose: bool,
) -> Result<(Plan, Option<PathBuf>), Error> {
    if !matches!(plan.entry(path)?, Entry::Directory) {
        return Ok((plan, None));
    }

    let mut entries = BTreeMap::new();
    for (name, child) in plan.read_directory(path)? {
        let source = match plan.entry(&child)? {
            Entry::Symlink(source) => source,
            Entry::Directory => {
                let (next_plan, source) = plan_refold(plan, &child, verbose)?;
                plan = next_plan;
                let Some(source) = source else {
                    return Ok((plan, None));
                };
                source
            }
            Entry::Missing | Entry::File => return Ok((plan, None)),
        };

        if !plan.is_owned_source(&source) {
            return Ok((plan, None));
        }
        entries.insert(name, (child, source));
    }

    if entries.is_empty() {
        return Ok((plan, None));
    }

    let Some(source_dir) = entries
        .values()
        .next()
        .and_then(|(_, source)| source.parent())
        .map(Path::to_path_buf)
    else {
        return Ok((plan, None));
    };

    if entries
        .values()
        .any(|(_, source)| source.parent() != Some(source_dir.as_path()))
    {
        return Ok((plan, None));
    }

    let source_entries = read_dir(&source_dir)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let target_entries = entries.keys().cloned().collect::<BTreeSet<_>>();
    if source_entries != target_entries {
        return Ok((plan, None));
    }

    for (name, (child, source)) in entries {
        if source.file_name() != Some(name.as_os_str()) {
            return Ok((plan, None));
        }
        plan = plan.remove_symlink(child);
    }
    plan = plan.remove_directory(path.to_path_buf());
    if verbose {
        println!("Planning directory refold: {:?} -> {:?}", path, source_dir);
    }
    plan = plan.create_symlink(path.to_path_buf(), &source_dir)?;

    Ok((plan, Some(source_dir)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all, remove_dir_all};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::commands::stow;
    use crate::fs::resolve_link;

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
            create_dir_all(root.join("stow")).unwrap();
            Self { root }
        }

        fn args(&self, package: &str) -> UnstowArgs {
            UnstowArgs {
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
    fn removes_a_folded_package_link() {
        let tree = TestTree::new();
        create_dir_all(tree.root.join("stow").join("package").join("bin")).unwrap();
        File::create(
            tree.root
                .join("stow")
                .join("package")
                .join("bin")
                .join("tool.exe"),
        )
        .unwrap();
        stow::run(&tree.args("package")).unwrap();

        run(&tree.args("package")).unwrap();

        assert!(!tree.root.join("bin").exists());
        assert!(!tree.root.join("bin").is_symlink());
    }

    #[test]
    fn refolds_directory_after_removing_another_package() {
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
        stow::run(&tree.args("old-package")).unwrap();
        stow::run(&tree.args("new-package")).unwrap();

        run(&tree.args("old-package")).unwrap();

        assert!(tree.root.join("bin").is_symlink());
        assert_eq!(
            resolve_link(&tree.root.join("bin")).unwrap(),
            tree.root
                .join("stow")
                .join("new-package")
                .join("bin")
                .canonicalize()
                .unwrap()
        );
    }
}
