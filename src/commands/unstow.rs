use std::env::current_dir;
use std::fs::read_link;
use std::path::{Path, PathBuf};

use crate::cli::UnstowArgs;
use crate::error::Error;
use crate::fs::{Base, Package, Symlink, Target, relative_path};

pub fn run(args: UnstowArgs) -> Result<(), Error> {
    if args.packages.is_empty() {
        eprintln!("error: At least one package is required");
        return Err(Error::MissingPackages);
    }

    let cwd = current_dir()?;
    let package_dir = args
        .package_dir
        .unwrap_or_else(|| cwd.clone())
        .canonicalize()?;

    let target_dir = args
        .target_dir
        .unwrap_or(cwd)
        .parent()
        .map_or_else(|| Err(Error::DefaultTargetNotAvailable), Ok)?
        .canonicalize()?;

    for pkg in args.packages.iter() {
        if args.verbose {
            println!("Unstowing package: {}", pkg);
        }

        let actions = do_unstow(&package_dir, &target_dir, &pkg, args.verbose)?;

        if args.simulate {
            for path in &actions {
                println!("Would remove symlink: {}", path.display());
            }
        } else {
            for path in &actions {
                if args.verbose {
                    println!("Removing symlink: {}", path.display());
                }
                // std::fs::remove_file(path)?;
                todo!("Actually remove the symlink: {:?}", path);
            }
        }
        if args.verbose {
            println!("Unstowed package: {}", pkg);
        }
    }

    Ok(())
}

fn do_unstow(
    package_dir: &Path,
    target_dir: &Path,
    pkg: &str,
    verbose: bool,
) -> Result<Vec<PathBuf>, Error> {
    let package_path = package_dir.join(pkg);
    let link_target_base = relative_path(Target(&package_path), Base(&target_dir))?;
    if verbose {
        println!("target base: {:?}", link_target_base);
    }

    let mut actions = Vec::new();

    let package = Package::new(package_dir, pkg)?;
    for item in package.get_package_contents()? {
        if verbose {
            println!("stow::run: Stowing item: {}", item.display());
        }

        let link_path = target_dir.join(&item);
        // this is the base directory of the link targets
        let link_target = link_target_base.join(&item);

        // It is ok if the link path already exists, if either:
        // - it is a directory
        // - it is a symlink that points to the same target

        if link_path.is_dir() {
            todo!("Handle existing directory at link path: {:?}", link_path);
            // Try to create symlinks into that directory such that
            // target/dir/item -> package/dir/item

            // Do this until all links have been created, or an error occurs.
        } else if link_path.is_symlink() {
            let existing_target = read_link(&link_path)?;
            if existing_target == link_target {
                if verbose {
                    println!(
                        "symlink({:?}, {:?}) exists, scheduling for removal",
                        link_path, link_target
                    );
                }
            } else {
                eprintln!(
                    "error: Link path {:?} is not owned by package '{}'. not removing it.",
                    link_path, pkg
                );
                continue;
            }
        } else if link_path.is_file() {
            eprintln!(
                "symlink({:?}, {:?}) exists and is not a directory or symlink. not removing it.",
                link_path, link_target
            );
            continue;
        }

        actions.push(link_path);
    }

    Ok(actions)
}

fn is_owned_by_package(package_dir: &Path, target_dir: &Path, pkg: &str) -> Result<bool, Error> {
    let package_path = package_dir.join(pkg);
    let link_target_base = relative_path(Target(&package_path), Base(&target_dir))?;
    let _ = link_target_base;

    // Check if the symlink points to the package directory
    // This is a placeholder for the actual implementation
    Ok(true)
}
